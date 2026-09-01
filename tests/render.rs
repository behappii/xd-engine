//! Интеграционные тесты: видят движок только через публичный API, как внешний
//! пользователь. `Scene::draw` пишет в обычные буферы и окна не требует,
//! поэтому кадр можно отрендерить прямо в тесте и разглядывать пиксели.

use std::collections::HashSet;

use xd_engine::{
    math::Vec3,
    scene::{Instance, Mesh, Scene},
    texture::Texture,
};

const WIDTH: u32 = 200;
const HEIGHT: u32 = 150;

fn render_at(scene: &Scene, width: u32, height: u32) -> Vec<u8> {
    let mut frame = vec![0u8; (width * height * 4) as usize];
    let mut depth = vec![0.0f32; (width * height) as usize];

    scene.draw(&mut frame, &mut depth, width, height);

    frame
}

fn render(scene: &Scene) -> Vec<u8> {
    render_at(scene, WIDTH, HEIGHT)
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

/// Куб, развёрнутый так, чтобы камера видела сразу три грани
fn tilted_cube(position: Vec3) -> Instance {
    let mut cube = Instance::new(Mesh::create_cube(), position).with_color([255, 255, 255, 255]);

    cube.rotation = Vec3::new(20.0, 35.0, 0.0);

    cube
}

#[test]
fn scene_with_a_cube_actually_draws_pixels() {
    let mut scene = Scene::new();
    scene.add_instance(tilted_cube(Vec3::new(0.0, 0.0, 0.0)));

    let frame = render(&scene);
    let painted = frame.chunks_exact(4).filter(|p| p[3] != 0).count();

    assert!(painted > 0, "кадр пустой");
}

#[test]
fn empty_scene_leaves_the_buffer_untouched() {
    let frame = render(&Scene::new());

    assert!(frame.iter().all(|byte| *byte == 0));
}

#[test]
fn flat_shading_gives_one_color_per_visible_face() {
    let mut scene = Scene::new();
    scene.add_instance(tilted_cube(Vec3::new(0.0, 0.0, 0.0)));

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

    let mut at_origin = Scene::new();
    at_origin.add_instance(tilted_cube(Vec3::new(0.0, 0.0, 0.0)));

    let mut far_away = Scene::new();
    far_away.add_instance(tilted_cube(offset));
    far_away.camera_position = at_origin.camera_position + offset;

    assert_eq!(
        face_colors(&render(&at_origin)),
        face_colors(&render(&far_away))
    );
}

#[test]
fn face_turned_towards_the_light_is_brighter_than_one_turned_away() {
    // Свет светит из (0.5, 1.0, 0.8) — сверху. Значит верхняя грань куба
    // должна быть светлее нижней, а не наоборот
    let mut scene = Scene::new();

    let mut cube = Instance::new(Mesh::create_cube(), Vec3::new(0.0, 0.0, 0.0))
        .with_color([255, 255, 255, 255]);
    // Смотрим на куб сверху, чтобы верхняя грань попала в кадр
    cube.rotation = Vec3::new(0.0, 0.0, 0.0);
    scene.add_instance(cube);
    scene.camera_position = Vec3::new(0.0, 5.0, 0.0001);
    scene.pitch = -89.0;
    scene.yaw = -90.0;

    let from_above = face_colors(&render(&scene));

    // Тот же куб снизу
    scene.camera_position = Vec3::new(0.0, -5.0, 0.0001);
    scene.pitch = 89.0;

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

fn sphere_scene(mesh: Mesh) -> Scene {
    let mut scene = Scene::new();

    let mut sphere = Instance::new(mesh, Vec3::new(0.0, 0.0, 0.0)).with_color([255, 255, 255, 255]);
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
    // smooth_shaded складывает ненормализованные векторные произведения
    // и нормализует только сумму — длина обязана получиться единичной
    for vertex in &Mesh::create_sphere(8, 12).vertices {
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
    let mut scene = Scene::new();
    scene.add_instance(tilted_cube(Vec3::new(0.0, 0.0, 0.0)));

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
    let mut scene = Scene::new();
    scene.add_instance(tilted_cube(Vec3::new(0.0, 0.0, 0.0)));

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
fn checker() -> Texture {
    Texture::checker(8, 2, [255, 255, 255, 255], [255, 0, 0, 255])
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
    let mut scene = Scene::new();
    scene.add_instance(tilted_cube(Vec3::new(0.0, 0.0, 0.0)).with_texture(checker()));

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

    let mut scene = Scene::new();
    scene.add_instance(
        Instance::new(Mesh::create_cube(), Vec3::new(0.0, 0.0, 0.0))
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
    let mut plain = Scene::new();
    plain.add_instance(tilted_cube(Vec3::new(0.0, 0.0, 0.0)));

    let mut textured = Scene::new();
    textured.add_instance(
        tilted_cube(Vec3::new(0.0, 0.0, 0.0)).with_texture(Texture::checker(
            4,
            1,
            [255, 255, 255, 255],
            [255, 255, 255, 255],
        )),
    );

    // Побайтовое совпадение всего кадра, а не только набора цветов
    assert_eq!(render(&plain), render(&textured));
}

#[test]
fn an_instance_without_a_texture_is_unaffected_by_its_neighbour() {
    // Текстура — состояние, которое сцена переключает между инстансами.
    // Забыть сбросить его — классическая ошибка «протёкшего» стейта:
    // следующий объект отрисовался бы чужой картинкой
    let mut alone = Scene::new();
    alone.add_instance(tilted_cube(Vec3::new(0.0, 0.0, 0.0)));

    let mut after_textured = Scene::new();
    // Текстурированный куб стоит далеко в стороне и в кадр не попадает —
    // важно только то, что он обрабатывается раньше
    after_textured.add_instance(tilted_cube(Vec3::new(-40.0, 0.0, 0.0)).with_texture(checker()));
    after_textured.add_instance(tilted_cube(Vec3::new(0.0, 0.0, 0.0)));

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

    let mut tiled = Scene::new();
    let mut instance = Instance::new(mesh, Vec3::new(0.0, 0.0, 0.0))
        .with_color([255, 255, 255, 255])
        .with_texture(checker());
    instance.rotation = Vec3::new(20.0, 35.0, 0.0);
    tiled.add_instance(instance);

    let mut plain = Scene::new();
    plain.add_instance(tilted_cube(Vec3::new(0.0, 0.0, 0.0)).with_texture(checker()));

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
fn sphere_shares_vertices_instead_of_splitting_them() {
    let sphere = Mesh::create_sphere(12, 16);

    // Полюса + кольца, без дублей: расщепление дало бы втрое больше вершин,
    // чем треугольников, а тут их наоборот меньше
    assert_eq!(sphere.vertices.len(), 2 + 11 * 16);
    assert!(sphere.vertices.len() < sphere.triangles.len());
}
