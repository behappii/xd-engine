//! Интеграционные тесты: видят движок только через публичный API, как внешний
//! пользователь. `Scene::draw` пишет в обычные буферы и окна не требует,
//! поэтому кадр можно отрендерить прямо в тесте и разглядывать пиксели.

use std::collections::HashSet;

use xd_engine::{
    math::Vec3,
    scene::{Instance, Mesh, Scene},
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

    assert_eq!(face_colors(&render(&at_origin)), face_colors(&render(&far_away)));
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

/// Размер закрашенной области в пикселях: (ширина, высота) габаритного
/// прямоугольника. Ноль на ноль, если не закрашено ничего
fn painted_size(frame: &[u8], width: u32) -> (u32, u32) {
    let painted: Vec<(u32, u32)> = frame
        .chunks_exact(4)
        .enumerate()
        .filter(|(_, p)| p[3] != 0)
        .map(|(i, _)| (i as u32 % width, i as u32 / width))
        .collect();

    if painted.is_empty() {
        return (0, 0);
    }

    let xs = painted.iter().map(|(x, _)| *x);
    let ys = painted.iter().map(|(_, y)| *y);

    // +1, потому что габарит из одного пикселя — это ширина 1, а не 0
    (
        xs.clone().max().unwrap() - xs.min().unwrap() + 1,
        ys.clone().max().unwrap() - ys.min().unwrap() + 1,
    )
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

#[test]
fn sphere_shares_vertices_instead_of_splitting_them() {
    let sphere = Mesh::create_sphere(12, 16);

    // Полюса + кольца, без дублей: расщепление дало бы втрое больше вершин,
    // чем треугольников, а тут их наоборот меньше
    assert_eq!(sphere.vertices.len(), 2 + 11 * 16);
    assert!(sphere.vertices.len() < sphere.triangles.len());
}
