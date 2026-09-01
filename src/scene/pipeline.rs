use rayon::prelude::*;

use crate::{
    clipping::clip_triangle_near,
    config::{
        AMBIENT_LIGHT, DEFAULT_FAR, DEFAULT_FOV, DEFAULT_NEAR, LIGHT_DIRECTION, LINE_COLOR,
        RASTER_BAND_ROWS,
    },
    math::{Mat4, Vec3, Vec4},
    renderer::{
        DrawContext, ShadedVertex, draw_triangle_filled, draw_triangle_wireframe, is_backface,
    },
    texture::Texture,
};

use super::{Assets, Instance, Scene};

/// Цвет из байтов в вектор с компонентами 0..1.
/// В таком виде его можно умножать на яркость и интерполировать
fn unpack_color(color: [u8; 4]) -> Vec3 {
    Vec3::new(
        color[0] as f32 / 255.0,
        color[1] as f32 / 255.0,
        color[2] as f32 / 255.0,
    )
}

/// Вершинный этап целиком: из инстансов получается плоский список
/// треугольников, готовых к растеризации.
///
/// Раньше этот код рисовал сразу, по ходу обхода инстансов. Разделение
/// нужно потому, что растеризация теперь идёт по полосам экрана, а полоса
/// не знает ничего про инстансы — ей нужен готовый список. Порядок списка
/// повторяет прежний порядок отрисовки, и это важно: тест глубины и
/// затирание проволочных линий зависят от порядка.
///
/// Сцену берёт параметром, а не через `self`: это уже не метод сцены, а вход
/// в конвейер, и держать его надо рядом с остальным конвейером. Иначе
/// `impl Scene` разъехался бы по двум файлам ради одной функции
pub(super) fn build_raster_jobs<'a>(
    scene: &Scene,
    assets: &'a Assets,
    width: u32,
    height: u32,
    parallel: bool,
) -> Vec<RasterJob<'a>> {
    // Рассчитываем текущие векторы направления камеры
    let yaw_rad = scene.yaw.to_radians();
    let pitch_rad = scene.pitch.to_radians();

    let forward = Vec3::new(
        yaw_rad.cos() * pitch_rad.cos(),
        pitch_rad.sin(),
        yaw_rad.sin() * pitch_rad.cos(),
    )
    .normalize();

    // Расчет матрицы Вида (Она едина для всей сцены)
    let target_pos = scene.camera_position + forward;
    let up_vector = Vec3::new(0.0, 1.0, 0.0);
    let view_matrix = Mat4::look_at(scene.camera_position, target_pos, up_vector);

    // отношение ширина:высота экрана
    let aspect = width as f32 / height as f32;

    // Рассчет матрицы проекции на экран
    let projection_matrix = Mat4::perspective(DEFAULT_FOV, aspect, DEFAULT_NEAR, DEFAULT_FAR);

    // Объединяем View * Projection один раз для кадра
    let vp_matrix = &projection_matrix * &view_matrix;

    // Направление на источник света — одно на всю сцену, считаем до циклов
    let light_dir = LIGHT_DIRECTION.normalize();

    // У каждого инстанса свой выходной вектор, и складываются они потом
    // строго по порядку инстансов. Это и есть весь секрет сохранения
    // порядка: как бы инстансы ни разошлись по потокам, склейка идёт по
    // индексу, а не по тому, кто раньше закончил. Порядок важен, потому
    // что от него зависят тест глубины и затирание проволочных линий
    let mut per_instance: Vec<Vec<RasterJob<'a>>> =
        (0..scene.instances.len()).map(|_| Vec::new()).collect();

    if parallel {
        // Именно здесь окупился отказ от Rc: в сцене не осталось ни одного
        // неатомарного счётчика ссылок, поэтому `&Scene` — Sync, и её можно
        // просто одолжить всем потокам.
        //
        // for_each_init, а не for_each: черновики создаются ОДИН раз на
        // рабочий поток и переиспользуются между его инстансами. Обычный
        // for_each выделял бы их заново на каждый объект каждый кадр
        scene
            .instances
            .par_iter()
            .zip(per_instance.par_iter_mut())
            .for_each_init(VertexScratch::default, |scratch, (instance, out)| {
                shade_instance(assets, instance, &vp_matrix, light_dir, scratch, out);
            });
    } else {
        let mut scratch = VertexScratch::default();

        for (instance, out) in scene.instances.iter().zip(per_instance.iter_mut()) {
            shade_instance(assets, instance, &vp_matrix, light_dir, &mut scratch, out);
        }
    }

    let mut jobs: Vec<RasterJob<'a>> = Vec::with_capacity(per_instance.iter().map(Vec::len).sum());

    for mut chunk in per_instance {
        jobs.append(&mut chunk);
    }

    jobs
}

/// Вершинный этап одного инстанса: из меша получаются готовые треугольники.
///
/// Свободная функция, а не метод сцены: со сцены здесь не нужно ничего, кроме
/// самого инстанса, — вся геометрия и текстуры лежат в аренах. Заодно это
/// видно из сигнатуры, и одинаково вызывается из однопоточной ветки и из потока
fn shade_instance<'a>(
    assets: &'a Assets,
    instance: &Instance,
    vp_matrix: &Mat4,
    light_dir: Vec3,
    scratch: &mut VertexScratch,
    out: &mut Vec<RasterJob<'a>>,
) {
    let mesh = assets.mesh(instance.mesh);

    let model_matrix = instance.get_model_matrix();
    let mvp_matrix = vp_matrix * &model_matrix;

    // Нормали едут НЕ по модельной матрице: при неравномерном масштабе она
    // поворачивает их не туда (подробности — в `Mat4::normal_matrix`).
    // Считается один раз на инстанс, а не на вершину: обращение матрицы стоит
    // порядка сотни операций, и в вершинном цикле это было бы заметно
    let normal_matrix = model_matrix.normal_matrix();

    // Текстура — состояние на весь инстанс, как и на GPU: она
    // «привязывается» один раз перед отрисовкой объекта. Здесь она всё же
    // копируется в каждый треугольник: список плоский, инстанс из него
    // уже не виден, а `Option<&Texture>` — это одно слово
    let texture = instance.texture.map(|id| assets.texture(id));

    // Вершинный этап: позиция уходит в clip space, а нормаль сразу
    // превращается в яркость. Это и есть затенение по Гуро — свет
    // считается в вершинах, дальше по грани его протянет интерполяция.
    // Сама нормаль ниже уже не нужна, поэтому и не храним её
    let VertexScratch {
        clip_vertices,
        intensities,
    } = scratch;

    clip_vertices.clear();
    intensities.clear();

    for vertex in &mesh.vertices {
        clip_vertices.push(&mvp_matrix * vertex.position);

        let normal = normal_matrix.transform_dir(vertex.normal).normalize();
        let lambert = normal.dot(&light_dir).max(0.0);

        intensities.push(AMBIENT_LIGHT + (1.0 - AMBIENT_LIGHT) * lambert);
    }

    // Отрисовываем грани этого меша с отсечением невидимых
    {
        for (i, triangle) in mesh.triangles.iter().enumerate() {
            let v0 = clip_vertices[triangle[0]];
            let v1 = clip_vertices[triangle[1]];
            let v2 = clip_vertices[triangle[2]];

            if is_backface(v0, v1, v2) {
                continue; // грань отвернута - пропускаем
            }

            // цвет грани до освещения
            // Если раскраски по граням нет (или она короче) — берём цвет объекта
            let base_color = instance
                .face_colors
                .as_ref()
                .and_then(|fc| fc.get(i))
                .copied()
                .unwrap_or(instance.color);

            // Если включен режим проволочных граней для инстанса.
            // Проволока режется по ближней плоскости не здесь, а внутри
            // самой отрисовки линии, поэтому в список едут исходные
            // clip-позиции
            if instance.wireframe {
                out.push(RasterJob::Wireframe {
                    positions: [v0, v1, v2],
                    color: base_color,
                });
                continue;
            }

            // Собираем вершины для растеризатора: у каждой свой цвет,
            // потому что своя яркость. У меша из flat_shaded все три
            // яркости совпадают и грань выходит однотонной, у гладкого —
            // расходятся, и интерполяция даёт градиент
            let base = unpack_color(base_color);
            let shaded = |index: usize| {
                ShadedVertex::new(clip_vertices[index], base * intensities[index])
                    .with_uv(mesh.vertices[index].uv)
            };

            // Режем по ближней плоскости; в список едут уже осколки
            let (triangles, count) = clip_triangle_near([
                shaded(triangle[0]),
                shaded(triangle[1]),
                shaded(triangle[2]),
            ]);

            for triangle in &triangles[..count] {
                out.push(RasterJob::Filled {
                    vertices: *triangle,
                    texture,
                });
            }
        }
    }
}

/// Переиспользуемые буферы вершинного этапа.
///
/// Один такой на поток, а не на инстанс: длина у них — число вершин меша, и
/// без переиспользования каждый объект каждый кадр заново выделял бы память
#[derive(Default)]
struct VertexScratch {
    clip_vertices: Vec<Vec4>,
    intensities: Vec<f32>,
}

/// Один готовый к растеризации треугольник.
///
/// Плоский список таких работ — это граница между вершинным этапом и
/// растеризацией. Всё, что нужно знать про инстанс, здесь уже скопировано:
/// потоку-растеризатору сцена не видна вообще
pub(super) enum RasterJob<'tex> {
    Filled {
        vertices: [ShadedVertex; 3],
        texture: Option<&'tex Texture>,
    },
    /// У проволоки нет ни атрибутов, ни теста глубины — только позиции и цвет
    Wireframe {
        positions: [Vec4; 3],
        color: [u8; 4],
    },
}

/// Полоса кадра, отданная одному потоку: номер первой строки и куски обоих
/// буферов, относящиеся только к ней
type Band<'a> = (u32, &'a mut [u8], &'a mut [f32]);

/// Растеризовать список треугольников в кадр, разложив работу по потокам.
///
/// Схема: кадр режется на горизонтальные полосы, полосы разбирает пул
/// потоков, каждая полоса проходит ВЕСЬ список треугольников и рисует только
/// то, что в неё попало.
///
/// Раздачей занимается work-stealing rayon, а не мы: поток, доевший свои
/// полосы, забирает чужие. Раньше здесь была статическая раздача по кругу —
/// она нужна была потому, что сплошной кусок экрана даёт неравномерную
/// нагрузку (небо сверху пустое, пол снизу закрашен целиком). Кража работы
/// решает ту же задачу лучше и без наших рук: перекос выравнивается по факту,
/// а не по догадке о том, где на экране будет тяжело.
///
/// Ключевое свойство — полосы не пересекаются. Значит два потока физически не
/// могут писать в один пиксель, и никакой синхронизации, атомиков и мьютексов
/// не нужно: `chunks_mut` выдаёт непересекающиеся `&mut`-срезы, и это
/// доказывает компилятор, а не комментарий.
///
/// Цена схемы — треугольник обходят все потоки, даже те, чьи полосы он не
/// задевает. Отсечение по Y стоит несколько сравнений, так что для крупных
/// граней это ничто, а вот на сцене из множества мелких треугольников
/// начнёт мешать: лечится разбиением экрана на плитки с предварительной
/// раскладкой треугольников по ним, но это заметно сложнее
pub(super) fn rasterize(
    frame: &mut [u8],
    depth: &mut [f32],
    width: u32,
    height: u32,
    jobs: &[RasterJob<'_>],
    parallel: bool,
) {
    if width == 0 || height == 0 {
        return;
    }

    let band_pixels = RASTER_BAND_ROWS * width as usize;

    let bands: Vec<Band<'_>> = frame
        .chunks_mut(band_pixels * 4)
        .zip(depth.chunks_mut(band_pixels))
        .enumerate()
        .map(|(i, (frame_band, depth_band))| {
            ((i * RASTER_BAND_ROWS) as u32, frame_band, depth_band)
        })
        .collect();

    if parallel {
        bands
            .into_par_iter()
            .for_each(|band| rasterize_band(band, jobs, width, height));
    } else {
        // Путь без единого потока: с ним сравнивается параллельный результат
        for band in bands {
            rasterize_band(band, jobs, width, height);
        }
    }
}

/// Пройти весь список треугольников для одной полосы
fn rasterize_band(band: Band<'_>, jobs: &[RasterJob<'_>], width: u32, height: u32) {
    let (y_offset, frame_band, depth_band) = band;

    // Высота берётся из длины среза, а не из RASTER_BAND_ROWS: последняя
    // полоса короче, если высота кадра не делится нацело
    let rows = depth_band.len() as u32 / width;

    let mut ctx = DrawContext::band(
        frame_band, depth_band, width, height, y_offset, rows, LINE_COLOR,
    );

    for job in jobs {
        match job {
            RasterJob::Filled { vertices, texture } => {
                ctx.texture = *texture;
                draw_triangle_filled(vertices[0], vertices[1], vertices[2], &mut ctx);
            }
            RasterJob::Wireframe { positions, color } => {
                ctx.color = *color;
                draw_triangle_wireframe(positions[0], positions[1], positions[2], &mut ctx);
            }
        }
    }
}
