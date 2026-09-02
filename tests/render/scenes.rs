//! Несколько сцен на общих ресурсах и наложение одной на другую.

use xd_engine::{
    math::Vec3,
    scene::{Assets, Instance, Mesh, MeshId, Scene},
};

use crate::harness::{HEIGHT, WIDTH};

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
