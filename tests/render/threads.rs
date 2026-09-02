//! Многопоточная растеризация: кадр обязан совпасть побайтово.

use xd_engine::{
    math::Vec3,
    scene::{Instance, Mesh},
    texture::Texture,
};

use crate::harness::{HEIGHT, WIDTH, World};

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

    // Проволока стоит ближе всех и рисуется последней: её пиксели ходят через
    // другой путь растеризатора, чем заливка, и режутся по полосам не по
    // габаритному прямоугольнику, а по-своему — по каждому шагу Брезенхема
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
