//! Интеграционные тесты: видят движок только через публичный API, как внешний
//! пользователь. `Scene::draw` пишет в обычные буферы и окна не требует,
//! поэтому кадр можно отрендерить прямо в тесте и разглядывать пиксели.

use std::collections::HashSet;

use xd_engine::{
    math::Vec3,
    scene::{Assets, Instance, Mesh, MeshId, Scene, TextureId},
    texture::Texture,
};

/// Сцена вместе со своими ресурсами.
///
/// Ресурсы и мир разделены в движке нарочно — арены переживают любую отдельную
/// сцену. Но в тестах они всегда ходят парой, поэтому держим их рядом, чтобы
/// каждый тест не таскал два значения и не забывал их сопоставлять
struct World {
    assets: Assets,
    scene: Scene,
}

impl World {
    fn new() -> Self {
        Self {
            assets: Assets::new(),
            scene: Scene::new(),
        }
    }

    fn add_mesh(&mut self, mesh: Mesh) -> MeshId {
        self.assets.add_mesh(mesh)
    }

    fn add_texture(&mut self, texture: Texture) -> TextureId {
        self.assets.add_texture(texture)
    }

    fn add_instance(&mut self, instance: Instance) {
        self.scene.add_instance(instance);
    }
}

const WIDTH: u32 = 200;
const HEIGHT: u32 = 150;

fn render_at(world: &World, width: u32, height: u32) -> Vec<u8> {
    let mut frame = vec![0u8; (width * height * 4) as usize];
    let mut depth = vec![0.0f32; (width * height) as usize];

    world
        .scene
        .draw(&world.assets, &mut frame, &mut depth, width, height);

    frame
}

fn render(world: &World) -> Vec<u8> {
    render_at(world, WIDTH, HEIGHT)
}

/// Цвета всех закрашенных пикселей. Фон — нули, его отбрасываем.
/// При плоском затенении каждая видимая грань даёт ровно один цвет,
/// так что размер множества — это число видимых граней
fn face_colors(frame: &[u8]) -> HashSet<[u8; 4]> {
    frame
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .filter(|p| *p != [0, 0, 0, 0])
        .collect()
}

/// Куб, развёрнутый так, чтобы камера видела сразу три грани.
///
/// Меш регистрируется в сцене прямо здесь: инстанс хранит только MeshId,
/// и без своей сцены он ничего не значит
fn tilted_cube(world: &mut World, position: Vec3) -> Instance {
    let mesh = world.add_mesh(Mesh::create_cube());
    let mut cube = Instance::new(mesh, position).with_color([255, 255, 255, 255]);

    cube.rotation = Vec3::new(20.0, 35.0, 0.0);

    cube
}

#[test]
fn scene_with_a_cube_actually_draws_pixels() {
    let mut scene = World::new();
    let cube = tilted_cube(&mut scene, Vec3::new(0.0, 0.0, 0.0));
    scene.add_instance(cube);

    let frame = render(&scene);
    let painted = frame.chunks_exact(4).filter(|p| p[3] != 0).count();

    assert!(painted > 0, "кадр пустой");
}

#[test]
fn empty_scene_leaves_the_buffer_untouched() {
    let frame = render(&World::new());

    assert!(frame.iter().all(|byte| *byte == 0));
}

#[test]
fn flat_shading_gives_one_color_per_visible_face() {
    let mut scene = World::new();
    let cube = tilted_cube(&mut scene, Vec3::new(0.0, 0.0, 0.0));
    scene.add_instance(cube);

    // Три видимые грани куба, три разных наклона к свету, три оттенка.
    // Если бы затенение считалось не по граням, оттенков было бы больше
    assert_eq!(face_colors(&render(&scene)).len(), 3);
}

#[test]
fn shading_does_not_depend_on_world_position() {
    // Регрессия на нормали, но уже через весь пайплайн целиком.
    //
    // Двигаем куб и камеру на один и тот же вектор: взаимное расположение
    // не меняется, значит и освещение обязано остаться прежним. Пока нормаль
    // умножалась как точка, к ней прибавлялась позиция объекта — и этот тест
    // разошёлся бы
    let offset = Vec3::new(20.0, -13.0, 7.0);

    let mut at_origin = World::new();
    let cube = tilted_cube(&mut at_origin, Vec3::new(0.0, 0.0, 0.0));
    at_origin.add_instance(cube);

    let mut far_away = World::new();
    let cube = tilted_cube(&mut far_away, offset);
    far_away.add_instance(cube);
    far_away.scene.camera_position = at_origin.scene.camera_position + offset;

    assert_eq!(
        face_colors(&render(&at_origin)),
        face_colors(&render(&far_away))
    );
}

#[test]
fn face_turned_towards_the_light_is_brighter_than_one_turned_away() {
    // Свет светит из (0.5, 1.0, 0.8) — сверху. Значит верхняя грань куба
    // должна быть светлее нижней, а не наоборот
    let mut scene = World::new();

    let cube_mesh = scene.add_mesh(Mesh::create_cube());
    let mut cube =
        Instance::new(cube_mesh, Vec3::new(0.0, 0.0, 0.0)).with_color([255, 255, 255, 255]);
    // Смотрим на куб сверху, чтобы верхняя грань попала в кадр
    cube.rotation = Vec3::new(0.0, 0.0, 0.0);
    scene.add_instance(cube);
    scene.scene.camera_position = Vec3::new(0.0, 5.0, 0.0001);
    scene.scene.pitch = -89.0;
    scene.scene.yaw = -90.0;

    let from_above = face_colors(&render(&scene));

    // Тот же куб снизу
    scene.scene.camera_position = Vec3::new(0.0, -5.0, 0.0001);
    scene.scene.pitch = 89.0;

    let from_below = face_colors(&render(&scene));

    let brightest = |colors: &HashSet<[u8; 4]>| colors.iter().map(|c| c[0]).max().unwrap();

    assert!(
        brightest(&from_above) > brightest(&from_below),
        "сверху {:?}, снизу {:?}",
        brightest(&from_above),
        brightest(&from_below)
    );
}

/// Тот же самый шар, но нормали посчитаны по-плоски: позиции и треугольники
/// берём у гладкого меша, меняется ровно один атрибут
fn faceted_copy(smooth: &Mesh) -> Mesh {
    let positions: Vec<Vec3> = smooth.vertices.iter().map(|v| v.position).collect();

    Mesh::flat_shaded(&positions, &smooth.triangles)
}

fn sphere_scene(mesh: Mesh) -> World {
    let mut scene = World::new();

    let id = scene.add_mesh(mesh);
    let mut sphere = Instance::new(id, Vec3::new(0.0, 0.0, 0.0)).with_color([255, 255, 255, 255]);
    sphere.scale = Vec3::new(1.6, 1.6, 1.6);

    scene.add_instance(sphere);
    scene
}

#[test]
fn gouraud_turns_facets_into_a_gradient() {
    // Геометрия одна и та же, отличаются только нормали. У плоского варианта
    // все три вершины грани дают одну яркость и грань выходит однотонной;
    // у гладкого яркости в вершинах разные, и интерполяция размазывает их
    let smooth = Mesh::create_sphere(12, 16);
    let faceted = faceted_copy(&smooth);

    let smooth_shades = face_colors(&render(&sphere_scene(smooth))).len();
    let faceted_shades = face_colors(&render(&sphere_scene(faceted))).len();

    assert!(
        smooth_shades > faceted_shades * 3,
        "градиента не вышло: гладкий дал {} оттенков, гранёный {}",
        smooth_shades,
        faceted_shades
    );
}

#[test]
fn faceted_mesh_has_no_more_shades_than_triangles() {
    // Проверка от обратного: при плоском затенении число оттенков ограничено
    // числом граней, потому что внутри грани интерполировать нечего
    let faceted = faceted_copy(&Mesh::create_sphere(12, 16));
    let triangle_count = faceted.triangles.len();

    assert!(face_colors(&render(&sphere_scene(faceted))).len() <= triangle_count);
}

#[test]
fn smooth_normals_stay_unit_length() {
    // smooth_shaded складывает ненормализованные векторные произведения (длина
    // такого вектора равна удвоенной площади грани, поэтому крупные грани
    // весят больше) и нормализует только сумму — длина обязана получиться
    // единичной. Ненормированная нормаль тихо испортила бы яркость по Ламберту.
    //
    // Меш тут собственный, а не сфера: сфера с некоторых пор берёт нормали из
    // формулы, а не усреднением, и этот путь больше не проверяет
    let positions = [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(2.0, 0.0, 0.0),
        Vec3::new(2.0, 2.0, 0.0),
        Vec3::new(0.0, 2.0, 0.0),
    ];

    let mesh = Mesh::smooth_shaded(&positions, &[[0, 1, 2], [0, 2, 3]]);

    for vertex in &mesh.vertices {
        assert!(
            (vertex.normal.length() - 1.0).abs() < 1e-4,
            "нормаль длины {}",
            vertex.normal.length()
        );
    }
}

/// Габаритный прямоугольник закрашенной области: (min_x, min_y, max_x, max_y),
/// границы включительно. `None`, если не закрашено ничего
fn painted_bounds(frame: &[u8], width: u32) -> Option<(u32, u32, u32, u32)> {
    let painted: Vec<(u32, u32)> = frame
        .chunks_exact(4)
        .enumerate()
        .filter(|(_, p)| p[3] != 0)
        .map(|(i, _)| (i as u32 % width, i as u32 / width))
        .collect();

    if painted.is_empty() {
        return None;
    }

    let xs = painted.iter().map(|(x, _)| *x);
    let ys = painted.iter().map(|(_, y)| *y);

    Some((
        xs.clone().min().unwrap(),
        ys.clone().min().unwrap(),
        xs.max().unwrap(),
        ys.max().unwrap(),
    ))
}

/// Размер закрашенной области в пикселях: (ширина, высота) габаритного
/// прямоугольника. Ноль на ноль, если не закрашено ничего
fn painted_size(frame: &[u8], width: u32) -> (u32, u32) {
    match painted_bounds(frame, width) {
        // +1, потому что габарит из одного пикселя — это ширина 1, а не 0
        Some((min_x, min_y, max_x, max_y)) => (max_x - min_x + 1, max_y - min_y + 1),
        None => (0, 0),
    }
}

#[test]
fn widening_the_viewport_adds_margins_instead_of_stretching() {
    // Регрессия на ресайз, но проверяемая без окна: до этого размер кадра
    // приходил из констант, и растягивание окна плющило картинку.
    //
    // Почему ожидание именно такое. В `perspective` горизонтальный масштаб
    // равен вертикальному, делённому на aspect = width / height. Дальше NDC
    // умножается обратно на width при переводе в пиксели — и width
    // сокращается начисто:
    //
    //     x_экр = X * scale_y * height / (2w) + width / 2
    //
    // Ширины в формуле не осталось, есть только высота. Значит вдвое более
    // широкий кадр обязан дать фигуру ТОГО ЖЕ размера в пикселях, просто
    // с большими полями по бокам. Если бы aspect брался из константы,
    // а не из настоящего размера кадра, фигура растянулась бы вдвое
    let mut scene = World::new();
    let cube = tilted_cube(&mut scene, Vec3::new(0.0, 0.0, 0.0));
    scene.add_instance(cube);

    let narrow = painted_size(&render_at(&scene, 200, 150), 200);
    let wide = painted_size(&render_at(&scene, 400, 150), 400);

    assert!(narrow.0 > 0, "куб не попал в кадр, проверять нечего");

    // Допуск в пиксель — на округление при попадании краёв фигуры в сетку
    assert!(
        narrow.0.abs_diff(wide.0) <= 1,
        "куб растянуло по горизонтали: {} пикс. против {}",
        narrow.0,
        wide.0
    );
    assert!(
        narrow.1.abs_diff(wide.1) <= 1,
        "высота не должна была измениться вовсе: {} пикс. против {}",
        narrow.1,
        wide.1
    );
}

#[test]
fn taller_viewport_scales_the_picture_with_it() {
    // Обратная половина того же правила: высота кадра в формуле осталась,
    // причём линейно. Удвоив высоту при том же угле обзора, мы обязаны
    // получить вдвое более крупный объект.
    //
    // Тест не дублирует предыдущий, а закрывает вертикаль, которую тот
    // структурно не видит: там оба кадра одной высоты. Проверено поломкой —
    // если зашить высоту в перевод NDC в пиксели константой, соседний тест
    // остаётся зелёным, а этот краснеет. Aspect же на вертикаль не влияет
    // вовсе, так что его поломки ловит именно соседний тест
    let mut scene = World::new();
    let cube = tilted_cube(&mut scene, Vec3::new(0.0, 0.0, 0.0));
    scene.add_instance(cube);

    let small = painted_size(&render_at(&scene, 200, 150), 200);
    let tall = painted_size(&render_at(&scene, 200, 300), 200);

    let ratio = tall.1 as f32 / small.1 as f32;

    assert!(
        (ratio - 2.0).abs() < 0.05,
        "высота фигуры выросла в {:.3} раза вместо 2 ({} пикс. против {})",
        ratio,
        tall.1,
        small.1
    );
}

/// Шахматка 2×2 клетки: белая и красная.
///
/// Клетки различаются оттенком, а не яркостью, и это принципиально для
/// точного счёта цветов. Умножение на белый тексель — это ×1, оно возвращает
/// ровно тот байт, что и без текстуры; умножение на чистый красный обнуляет
/// два канала и оставляет тот же байт в третьем. Никаких новых округлений
/// не появляется, поэтому набор цветов предсказуем побайтово.
///
/// Первая версия теста брала тёмно-серую клетку и ломалась: неосвещённая
/// грань светится ровно фоновой подсветкой AMBIENT_LIGHT = 0.25, а 0.25 от
/// 60/255 — это точно 15/255, то есть граница между двумя байтами. Половина
/// пикселей грани падала по одну сторону, половина по другую, и «6 оттенков»
/// превращались в 7
fn checker(world: &mut World) -> TextureId {
    world.add_texture(Texture::checker(
        8,
        2,
        [255, 255, 255, 255],
        [255, 0, 0, 255],
    ))
}

#[test]
fn texture_multiplies_the_number_of_shades_by_the_number_of_texels() {
    // Развёртка куба отдаёт каждой грани весь квадрат текстуры, поэтому на
    // каждой из трёх видимых граней встречаются обе клетки шахматки. Свет
    // текстуру не заменяет, а модулирует — значит оттенков должно стать
    // ровно вдвое больше: 3 грани × 2 клетки.
    //
    // Тест ловит сразу две ошибки. Если UV не доехало до растеризатора, вся
    // грань прочитает один тексель и оттенков останется 3 (столько же, сколько
    // насчитал flat_shading_gives_one_color_per_visible_face). Если текстура
    // затирает свет вместо умножения, оттенков станет 2 — по числу клеток
    let mut scene = World::new();
    let texture = checker(&mut scene);
    let cube = tilted_cube(&mut scene, Vec3::new(0.0, 0.0, 0.0)).with_texture(texture);
    scene.add_instance(cube);

    assert_eq!(face_colors(&render(&scene)).len(), 6);
}

#[test]
fn the_cube_unwrap_puts_the_texture_upright_on_the_face() {
    // Четвёрки углов в `create_cube` выведены руками: обход против часовой
    // стрелки снаружи — чтобы нормаль смотрела наружу, начало с левого нижнего
    // угла — чтобы картинка стояла ровно. Первое условие ловят тесты нормалей,
    // а второе не поймает ничего: перепутанный стартовый угол просто повернёт
    // текстуру, и на шахматке это вообще не видно.
    //
    // Поэтому текстура здесь несимметричная: четыре разных цвета по четвертям.
    // Камера смотрит на грань +Z в упор, и каждая четверть картинки обязана
    // оказаться в своей четверти экрана
    let texture = Texture::from_rgba8(
        2,
        2,
        &[
            255, 0, 0, 255, // верх-лево  — красный
            0, 255, 0, 255, // верх-право — зелёный
            0, 0, 255, 255, // низ-лево   — синий
            255, 255, 255, 255, // низ-право  — белый
        ],
    );

    let mut scene = World::new();
    let mesh = scene.add_mesh(Mesh::create_cube());
    let texture = scene.add_texture(texture);
    scene.add_instance(
        Instance::new(mesh, Vec3::new(0.0, 0.0, 0.0))
            .with_color([255, 255, 255, 255])
            .with_texture(texture),
    );

    let frame = render(&scene);

    // Грань занимает середину кадра; отступаем от центра на четверть,
    // чтобы гарантированно попасть внутрь нужной четверти
    let pixel = |x: u32, y: u32| {
        let i = ((y * WIDTH + x) * 4) as usize;
        [frame[i], frame[i + 1], frame[i + 2]]
    };

    // Какой канал ярче — тот цвет и лежит в этом пикселе
    let dominant = |p: [u8; 3]| {
        if p[0] > 0 && p[1] == 0 && p[2] == 0 {
            "красный"
        } else if p[1] > 0 && p[0] == 0 && p[2] == 0 {
            "зелёный"
        } else if p[2] > 0 && p[0] == 0 && p[1] == 0 {
            "синий"
        } else if p[0] > 0 && p[1] > 0 && p[2] > 0 {
            "белый"
        } else {
            "пусто"
        }
    };

    // Точки берём по четвертям РЕАЛЬНОГО габарита грани, а не по четвертям
    // кадра: куб занимает лишь его середину, и фиксированный отступ от центра
    // легко промахнулся бы мимо фигуры в фон
    let (min_x, min_y, max_x, max_y) =
        painted_bounds(&frame, WIDTH).expect("куб не попал в кадр, проверять нечего");

    let left = min_x + (max_x - min_x) / 4;
    let right = max_x - (max_x - min_x) / 4;
    let top = min_y + (max_y - min_y) / 4;
    let bottom = max_y - (max_y - min_y) / 4;

    assert_eq!(dominant(pixel(left, top)), "красный", "верх-лево");
    assert_eq!(dominant(pixel(right, top)), "зелёный", "верх-право");
    assert_eq!(dominant(pixel(left, bottom)), "синий", "низ-лево");
    assert_eq!(dominant(pixel(right, bottom)), "белый", "низ-право");
}

#[test]
fn a_white_texture_changes_nothing() {
    // Умножение на единицу обязано быть тождественным. Тест сторожит именно
    // то, что текстурная ветка не делает с цветом ничего лишнего — например,
    // не теряет перспективную коррекцию и не подмешивает собственную яркость
    let mut plain = World::new();
    let cube = tilted_cube(&mut plain, Vec3::new(0.0, 0.0, 0.0));
    plain.add_instance(cube);

    let mut textured = World::new();
    let white = textured.add_texture(Texture::checker(
        4,
        1,
        [255, 255, 255, 255],
        [255, 255, 255, 255],
    ));
    let cube = tilted_cube(&mut textured, Vec3::new(0.0, 0.0, 0.0)).with_texture(white);
    textured.add_instance(cube);

    // Побайтовое совпадение всего кадра, а не только набора цветов
    assert_eq!(render(&plain), render(&textured));
}

#[test]
fn an_instance_without_a_texture_is_unaffected_by_its_neighbour() {
    // Текстура — состояние, которое сцена переключает между инстансами.
    // Забыть сбросить его — классическая ошибка «протёкшего» стейта:
    // следующий объект отрисовался бы чужой картинкой
    let mut alone = World::new();
    let cube = tilted_cube(&mut alone, Vec3::new(0.0, 0.0, 0.0));
    alone.add_instance(cube);

    let mut after_textured = World::new();
    // Текстурированный куб стоит далеко в стороне и в кадр не попадает —
    // важно только то, что он обрабатывается раньше
    let texture = checker(&mut after_textured);
    let far = tilted_cube(&mut after_textured, Vec3::new(-40.0, 0.0, 0.0)).with_texture(texture);
    after_textured.add_instance(far);
    let near = tilted_cube(&mut after_textured, Vec3::new(0.0, 0.0, 0.0));
    after_textured.add_instance(near);

    assert_eq!(
        face_colors(&render(&alone)),
        face_colors(&render(&after_textured))
    );
}

#[test]
fn uv_beyond_one_tiles_the_texture() {
    // Развёртка куба лежит в 0..1, но UV можно масштабировать — тогда режим
    // repeat раскладывает картинку плиткой. Проверяем, что координаты за
    // пределами квадрата не обрезаются в край и не паникуют: у растянутой
    // вчетверо шахматки клеток на грани должно стать заметно больше,
    // а набор цветов — остаться прежним
    let mut mesh = Mesh::create_cube();

    for vertex in &mut mesh.vertices {
        vertex.uv = vertex.uv * 4.0;
    }

    let mut tiled = World::new();
    let tiled_mesh = tiled.add_mesh(mesh);
    let tiled_texture = checker(&mut tiled);
    let mut instance = Instance::new(tiled_mesh, Vec3::new(0.0, 0.0, 0.0))
        .with_color([255, 255, 255, 255])
        .with_texture(tiled_texture);
    instance.rotation = Vec3::new(20.0, 35.0, 0.0);
    tiled.add_instance(instance);

    let mut plain = World::new();
    let plain_texture = checker(&mut plain);
    let cube = tilted_cube(&mut plain, Vec3::new(0.0, 0.0, 0.0)).with_texture(plain_texture);
    plain.add_instance(cube);

    // Цвета те же самые — меняется только их раскладка по поверхности
    assert_eq!(face_colors(&render(&tiled)), face_colors(&render(&plain)));

    // А вот переходов между клетками вдоль строки стало больше
    assert!(
        colour_switches_in_row(&render(&tiled), 75) > colour_switches_in_row(&render(&plain), 75)
    );
}

/// Сколько раз цвет меняется вдоль строки кадра. Грубая мера «дробности»
/// узора: чем мельче клетки, тем больше переходов
fn colour_switches_in_row(frame: &[u8], y: u32) -> usize {
    let row_start = (y * WIDTH * 4) as usize;
    let row = &frame[row_start..row_start + (WIDTH * 4) as usize];

    row.chunks_exact(4)
        .zip(row.chunks_exact(4).skip(1))
        .filter(|(a, b)| a != b)
        .count()
}

#[test]
fn sphere_splits_the_seam_so_the_texture_can_close() {
    let (stacks, slices) = (12, 16);
    let sphere = Mesh::create_sphere(stacks, slices);

    // Сетка с лишним столбцом (шов) и лишней строкой (полюса). Раньше сфера
    // замыкала кольцо остатком от деления, и вершин было 2 + 11 * 16
    assert_eq!(sphere.vertices.len(), (stacks + 1) * (slices + 1));

    // Концы кольца посередине сферы: одна и та же точка пространства, но на
    // картинке — левый и правый края развёртки
    let row = (stacks / 2) * (slices + 1);
    let left = sphere.vertices[row];
    let right = sphere.vertices[row + slices];

    assert!(
        (left.position - right.position).length() < 1e-5,
        "концы кольца разъехались: {:?} и {:?}",
        left.position,
        right.position
    );
    assert_eq!(left.uv.x, 0.0);
    assert_eq!(right.uv.x, 1.0);

    // Главное условие, из-за которого развёртки у сферы долго не было: у
    // близнецов на шве обязана быть ОДНА И ТА ЖЕ нормаль. Раньше нормали
    // считались усреднением по сходящимся граням, и каждой копии досталась бы
    // половина соседей — по сфере пошла бы видимая полоса
    assert!(
        (left.normal - right.normal).length() < 1e-5,
        "нормали на шве разошлись: {:?} и {:?}",
        left.normal,
        right.normal
    );
}

#[test]
fn sphere_normals_are_the_positions_themselves() {
    // У сферы единичного радиуса нормаль в точке известна точно и совпадает с
    // самой позицией. Усреднение по граням это лишь приближает — и именно оно
    // мешало дублировать вершины на шве
    for vertex in &Mesh::create_sphere(8, 12).vertices {
        assert!(
            (vertex.normal - vertex.position).length() < 1e-5,
            "нормаль {:?} не совпала с позицией {:?}",
            vertex.normal,
            vertex.position
        );
    }
}

#[test]
fn the_pole_cap_takes_the_middle_of_its_slice_not_the_edge() {
    // У полюса четырёхугольник вырожден в треугольник, и его вершине годится
    // любое u внутри доли — геометрия от этого не меняется. Но край доли даёт
    // в текстуре прямоугольный треугольник вместо равнобедренного, и картинка
    // у полюсов заметно косит. Ошибка совершенно тихая: ни нормали, ни обход,
    // ни силуэт от неё не страдают, поймать её может только эта проверка
    let sphere = Mesh::create_sphere(4, 8);

    let [apex, right, left] = sphere.triangles[0];
    let apex = sphere.vertices[apex];
    let (left, right) = (sphere.vertices[left], sphere.vertices[right]);

    assert!(
        (apex.position.y - 1.0).abs() < 1e-5,
        "первый треугольник должен быть северной шапкой"
    );

    let middle = (left.uv.x + right.uv.x) / 2.0;

    assert!(
        (apex.uv.x - middle).abs() < 1e-5,
        "вершина шапки получила u = {}, а середина доли — {middle}",
        apex.uv.x
    );
}

#[test]
fn the_sphere_unwrap_puts_the_top_of_the_texture_on_the_north_pole() {
    // Текстура из двух строк: верхняя красная, нижняя синяя. v = 0 — верхняя
    // строка картинки, северный полюс сферы — тоже v = 0, значит красное
    // обязано лечь сверху. Ширина 1 — чтобы u ни на что не влиял и тест
    // проверял ровно одно.
    //
    // Перевёрнутая по вертикали развёртка — ошибка, которой на однотонной или
    // симметричной текстуре не видно вовсе
    let texture = Texture::from_rgba8(1, 2, &[255, 0, 0, 255, /**/ 0, 0, 255, 255]);

    let mut scene = World::new();
    let mesh = scene.add_mesh(Mesh::create_sphere(16, 24));
    let texture = scene.add_texture(texture);
    let mut sphere = Instance::new(mesh, Vec3::new(0.0, 0.0, 0.0))
        .with_color([255, 255, 255, 255])
        .with_texture(texture);
    sphere.scale = Vec3::new(1.6, 1.6, 1.6);
    scene.add_instance(sphere);

    let frame = render(&scene);
    let (min_x, min_y, max_x, max_y) =
        painted_bounds(&frame, WIDTH).expect("сфера не нарисовалась");

    let pixel = |x: u32, y: u32| {
        let i = ((y * WIDTH + x) * 4) as usize;
        [frame[i], frame[i + 1], frame[i + 2]]
    };

    // Отступаем от края силуэта на шестую часть высоты: там уже видна лицевая
    // поверхность, а не касательный к камере край
    let step = (max_y - min_y) / 6;
    let center_x = (min_x + max_x) / 2;

    let top = pixel(center_x, min_y + step);
    let bottom = pixel(center_x, max_y - step);

    assert!(
        top[0] > top[2],
        "у северного полюса ждали красное, вышло {top:?}"
    );
    assert!(
        bottom[2] > bottom[0],
        "у южного полюса ждали синее, вышло {bottom:?}"
    );
}

/// Кадр вместе с буфером глубины, посчитанный заданным числом потоков.
///
/// Глубина сравнивается по битовому представлению, а не по значению: у f32
/// NaN не равен сам себе, и обычное сравнение молча пропустило бы кадр,
/// в котором расползлись именно они
fn render_threads(world: &World, width: u32, height: u32, threads: usize) -> (Vec<u8>, Vec<u32>) {
    let mut frame = vec![0u8; (width * height * 4) as usize];
    let mut depth = vec![0.0f32; (width * height) as usize];

    world.scene.draw_with_threads(
        &world.assets,
        &mut frame,
        &mut depth,
        width,
        height,
        threads,
    );

    (frame, depth.iter().map(|d| d.to_bits()).collect())
}

/// То же, но через обычный `draw` — то есть на глобальном пуле rayon
fn render_pooled(world: &World, width: u32, height: u32) -> (Vec<u8>, Vec<u32>) {
    let mut frame = vec![0u8; (width * height * 4) as usize];
    let mut depth = vec![0.0f32; (width * height) as usize];

    world
        .scene
        .draw(&world.assets, &mut frame, &mut depth, width, height);

    (frame, depth.iter().map(|d| d.to_bits()).collect())
}

/// Сцена, в которой одновременно работают все три пути растеризатора:
/// залитый треугольник, залитый С ТЕКСТУРОЙ и проволочный. Плюс перекрытия,
/// чтобы тест глубины действительно что-то решал
fn busy_scene() -> World {
    let mut scene = World::new();

    // Один меш на два инстанса — теперь это просто копия числа
    let cube = scene.add_mesh(Mesh::create_cube());
    let checker = scene.add_texture(Texture::checker(8, 4, [230; 4], [40, 40, 60, 255]));

    let mut textured = Instance::new(cube, Vec3::new(-1.2, 0.0, 0.0))
        .with_color([255, 255, 255, 255])
        .with_texture(checker);
    textured.rotation = Vec3::new(20.0, 35.0, 0.0);
    scene.add_instance(textured);

    let sphere_mesh = scene.add_mesh(Mesh::create_sphere(12, 16));
    let mut sphere =
        Instance::new(sphere_mesh, Vec3::new(1.2, 0.3, 0.0)).with_color([120, 190, 255, 255]);
    sphere.scale = Vec3::new(0.8, 0.8, 0.8);
    scene.add_instance(sphere);

    // Проволока идёт последней и пишет поверх без теста глубины — то есть
    // зависит от порядка. Если многопоточность его нарушит, будет видно
    let mut wire = Instance::new(cube, Vec3::new(0.0, -0.4, 1.2))
        .with_color([0, 255, 0, 255])
        .as_wireframe();
    wire.rotation = Vec3::new(10.0, 25.0, 0.0);
    scene.add_instance(wire);

    // Пол во всю нижнюю половину: он один пересекает добрую дюжину полос
    let mut floor_mesh = Mesh::create_cube();
    for vertex in &mut floor_mesh.vertices {
        vertex.uv = vertex.uv * 6.0;
    }
    let floor_id = scene.add_mesh(floor_mesh);
    let floor_texture = scene.add_texture(Texture::checker(8, 2, [200; 4], [60, 60, 80, 255]));
    let mut floor = Instance::new(floor_id, Vec3::new(0.0, -2.5, 0.0))
        .with_color([255, 255, 255, 255])
        .with_texture(floor_texture);
    floor.scale = Vec3::new(12.0, 0.2, 12.0);
    scene.add_instance(floor);

    scene
}

#[test]
fn threads_do_not_change_a_single_byte_of_the_frame() {
    // Главная проверка всей многопоточности. Полосы не пересекаются, каждый
    // пиксель пишет ровно один поток, а порядок треугольников внутри полосы
    // тот же, что и без потоков, — значит совпадение обязано быть точным.
    // Любое расхождение здесь означает гонку, а не «погрешность»
    let scene = busy_scene();

    let (base_frame, base_depth) = render_threads(&scene, WIDTH, HEIGHT, 1);

    assert!(
        base_frame.chunks_exact(4).any(|p| p[3] != 0),
        "сцена пустая — тест ничего не проверяет"
    );

    // Числа нарочно разные и не круглые: 3 и 5 не делят число полос нацело,
    // а 64 заведомо больше, чем полос в кадре, — часть потоков останется без
    // работы, и это не должно ничего сломать
    for threads in [2, 3, 5, 8, 64] {
        let (frame, depth) = render_threads(&scene, WIDTH, HEIGHT, threads);

        assert!(frame == base_frame, "кадр разошёлся на {threads} потоках");
        assert!(
            depth == base_depth,
            "глубина разошлась на {threads} потоках"
        );
    }

    // И то же самое через обычный draw, на глобальном пуле. Повтор здесь не
    // лишний: work-stealing раскладывает полосы каждый раз по-разному, в
    // зависимости от того, какой поток когда освободился. Значит один и тот же
    // вызов — это каждый раз новое расписание, и гонка, не проявившаяся в
    // первом прогоне, вполне может вылезти в пятом
    for run in 0..5 {
        let (frame, depth) = render_pooled(&scene, WIDTH, HEIGHT);

        assert!(frame == base_frame, "кадр разошёлся на пуле, прогон {run}");
        assert!(
            depth == base_depth,
            "глубина разошлась на пуле, прогон {run}"
        );
    }
}

#[test]
fn a_frame_height_that_is_not_a_multiple_of_the_band_still_matches() {
    // Полоса высотой RASTER_BAND_ROWS = 16 строк, и делить на неё высоту
    // нацело никто не обязан. Последняя полоса выходит короче, и её высоту
    // надо брать по факту, а не по константе, иначе она либо залезет за
    // границу буфера, либо не дорисует нижние строки
    let scene = busy_scene();

    // 16 — ровно полоса, 17 и 45 — с остатком, 7 — меньше одной полосы
    for height in [7, 16, 17, 45, 150] {
        let (single, single_depth) = render_threads(&scene, WIDTH, height, 1);
        let (many, many_depth) = render_threads(&scene, WIDTH, height, 8);

        assert!(single == many, "кадр разошёлся на высоте {height}");
        assert!(
            single_depth == many_depth,
            "глубина разошлась на высоте {height}"
        );
    }
}

#[test]
fn instances_are_drawn_in_the_order_they_were_added() {
    // Порядок инстансов — это порядок отрисовки, и вершинный этап обязан его
    // сохранять, хотя инстансы теперь расходятся по потокам как попало.
    // Держится это на том, что списки треугольников склеиваются по индексу
    // инстанса, а не по тому, кто раньше закончил.
    //
    // Проверка от проволоки: она пишет пиксели БЕЗ теста глубины, поэтому
    // видна только если рисуется последней. Мелкий проволочный куб целиком
    // накрыт залитым — стоит порядку сбиться, и от зелёного не останется
    // ни пикселя.
    //
    // Побайтовое сравнение потоков такую ошибку не поймает: перестановка
    // применилась бы одинаково и к однопоточной ветке, и к многопоточной,
    // и кадры остались бы равны друг другу — просто оба неправильные
    let mut scene = World::new();

    let filled = tilted_cube(&mut scene, Vec3::new(0.0, 0.0, 0.0));
    scene.add_instance(filled);

    let wire_mesh = scene.add_mesh(Mesh::create_cube());
    let mut wire = Instance::new(wire_mesh, Vec3::new(0.0, 0.0, 0.0))
        .with_color([0, 255, 0, 255])
        .as_wireframe();
    wire.rotation = Vec3::new(20.0, 35.0, 0.0);
    wire.scale = Vec3::new(0.4, 0.4, 0.4);
    scene.add_instance(wire);

    let green = |frame: &[u8]| {
        frame
            .chunks_exact(4)
            .filter(|p| **p == [0, 255, 0, 255])
            .count()
    };

    let frame = render(&scene);

    assert!(
        green(&frame) > 0,
        "проволоки не видно: значит её нарисовали ДО залитого куба"
    );
}

/// Куб заданного цвета и размера в заданной точке
fn coloured_cube(mesh: MeshId, position: Vec3, scale: f32, color: [u8; 4]) -> Instance {
    let mut cube = Instance::new(mesh, position).with_color(color);
    cube.scale = Vec3::new(scale, scale, scale);
    cube
}

/// Цвет пикселя в середине кадра
fn center_pixel(frame: &[u8]) -> [u8; 3] {
    let i = (((HEIGHT / 2) * WIDTH + WIDTH / 2) * 4) as usize;
    [frame[i], frame[i + 1], frame[i + 2]]
}

#[test]
fn two_scenes_share_one_set_of_assets() {
    // Ради этого ресурсы и отделены от сцены. Главное меню и уровень — это две
    // сцены, но куб в них один и тот же, и пересоздавать его при переходе
    // незачем. Пока арены жили внутри сцены, `MeshId` был действителен только
    // в своей: индекс из чужой сцены молча брал не тот меш, а то и выходил
    // за границу
    let mut assets = Assets::new();
    let cube = assets.add_mesh(Mesh::create_cube());

    // Один и тот же MeshId уходит в две независимые сцены
    let mut first = Scene::new();
    first.add_instance(coloured_cube(
        cube,
        Vec3::new(0.0, 0.0, 0.0),
        0.6,
        [255, 255, 255, 255],
    ));

    let mut second = Scene::new();
    second.add_instance(coloured_cube(
        cube,
        Vec3::new(0.0, 0.0, 0.0),
        0.6,
        [255, 255, 255, 255],
    ));

    let render = |scene: &Scene| {
        let mut frame = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
        let mut depth = vec![0.0f32; (WIDTH * HEIGHT) as usize];
        scene.draw(&assets, &mut frame, &mut depth, WIDTH, HEIGHT);
        frame
    };

    let a = render(&first);
    let b = render(&second);

    assert!(
        a.chunks_exact(4).any(|p| p[3] != 0),
        "сцена пустая — тест ничего не проверяет"
    );
    assert!(
        a == b,
        "один и тот же меш в двух сценах дал разную картинку"
    );
}

#[test]
fn a_second_scene_can_be_drawn_over_the_first() {
    // Внутриигровое меню поверх работающей игры. Отдельного механизма для
    // этого не нужно: `draw` пишет в переданные буферы, поэтому в один кадр
    // можно нарисовать сколько угодно сцен подряд. Мир при этом не трогают —
    // он просто не получает свой update и замирает.
    //
    // Единственная тонкость — глубина. Меню специально стоит ДАЛЬШЕ мира, и
    // без очистки depth-буфера тест глубины его отбракует. Очистка между
    // сценами и есть то, что делает вторую сцену наложением, а не частью мира
    let mut assets = Assets::new();
    let cube = assets.add_mesh(Mesh::create_cube());

    let mut world = Scene::new();
    world.add_instance(coloured_cube(
        cube,
        Vec3::new(0.0, 0.0, 0.0),
        0.6,
        [255, 255, 255, 255],
    ));

    // Дальше по Z, но крупнее — на экране накрывает мир целиком
    let mut menu = Scene::new();
    menu.add_instance(coloured_cube(
        cube,
        Vec3::new(0.0, 0.0, -3.0),
        1.5,
        [255, 0, 0, 255],
    ));

    let mut frame = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    let mut depth = vec![0.0f32; (WIDTH * HEIGHT) as usize];

    world.draw(&assets, &mut frame, &mut depth, WIDTH, HEIGHT);
    menu.draw(&assets, &mut frame, &mut depth, WIDTH, HEIGHT);

    let hidden = center_pixel(&frame);
    assert!(
        hidden[0] == hidden[1] && hidden[1] == hidden[2],
        "без очистки глубины меню обязано остаться за миром, а вышло {hidden:?}"
    );

    // Теперь то же самое, но глубина сбрасывается между сценами
    let mut frame = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    let mut depth = vec![0.0f32; (WIDTH * HEIGHT) as usize];

    world.draw(&assets, &mut frame, &mut depth, WIDTH, HEIGHT);
    depth.fill(0.0);
    menu.draw(&assets, &mut frame, &mut depth, WIDTH, HEIGHT);

    let shown = center_pixel(&frame);
    assert!(
        shown[0] > 0 && shown[1] == 0 && shown[2] == 0,
        "после очистки глубины меню обязано лечь поверх, а вышло {shown:?}"
    );
}
