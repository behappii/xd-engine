//! Переключение сцен по клавише `S`: `cargo run --release --example scenes`.
//!
//! Сцена 1 — два куба, сцена 2 — шар и пирамида. Показывает, ради чего
//! ресурсы отделены от мира: меши и текстура заводятся ОДИН раз, обе сцены
//! ими пользуются, а переключение не создаёт и не уничтожает ничего.
//!
//! Стека экранов и менеджера сцен в движке нет намеренно: какие бывают
//! экраны и что за чем следует — правила конкретной игры. Движок обязан лишь
//! сделать это дешёвым, и здесь видно, что этого достаточно.

// winit берётся через реэкспорт движка, а не своей зависимостью:
// так его версия заведомо совпадает с той, на которой собран xd_engine
use xd_engine::{
    KeyCode,
    app::EngineApp,
    math::Vec3,
    scene::{Mesh, MeshId, Scene, TextureId},
    texture::Texture,
    winit::event_loop::EventLoop,
};

/// Сцена 1: два куба, один текстурированный
fn two_cubes(cube: MeshId, checker: TextureId) -> Scene {
    let mut scene = Scene::new();

    // spawn заводит инстанс и сразу отдаёт ссылку на него — масштаб и цвет
    // это поля, а не методы-строители, и без ссылки пришлось бы заводить
    // временную переменную ради двух присваиваний
    let textured = scene.spawn(cube, Vec3::new(-1.3, 0.0, 0.0));
    textured.scale = Vec3::new(0.6, 0.6, 0.6);
    // Белый не случайно: тексель на цвет умножается, любой другой сработал бы
    // как светофильтр поверх картинки
    textured.color = [255, 255, 255, 255];
    textured.texture = Some(checker);

    let plain = scene.spawn(cube, Vec3::new(1.3, 0.0, 0.0));
    plain.scale = Vec3::new(0.6, 0.6, 0.6);
    plain.color = [120, 190, 255, 255];

    scene
}

/// Сцена 2: шар и пирамида
fn sphere_and_pyramid(sphere: MeshId, pyramid: MeshId, checker: TextureId) -> Scene {
    let mut scene = Scene::new();

    let ball = scene.spawn(sphere, Vec3::new(-1.3, 0.0, 0.0));
    ball.scale = Vec3::new(0.8, 0.8, 0.8);
    ball.color = [255, 255, 255, 255];
    ball.texture = Some(checker);

    let spike = scene.spawn(pyramid, Vec3::new(1.3, -0.3, 0.0));
    spike.scale = Vec3::new(0.7, 0.9, 0.7);
    spike.color = [255, 180, 90, 255];

    // Камера — часть сцены, а не движка, поэтому переключение меняет и точку
    // обзора. Здесь она поднята и отодвинута, чтобы разницу было видно сразу
    scene.camera_position = Vec3::new(0.0, 1.5, 6.0);
    scene.pitch = -12.0;

    scene
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    let mut app = EngineApp::new();

    // Ресурсы заводятся один раз и достаются обеим сценам. Ради этого арены и
    // вынесены из Scene: пока они лежали внутри, смена сцены выбрасывала все
    // меши и текстуры, чтобы тут же создать их заново
    let cube = app.assets.add_mesh(Mesh::create_cube());
    let sphere = app.assets.add_mesh(Mesh::create_sphere(16, 24));
    let pyramid = app.assets.add_mesh(Mesh::create_pyramid());
    let checker = app.assets.add_texture(Texture::checker(
        64,
        8,
        [230, 230, 230, 255],
        [40, 40, 60, 255],
    ));

    app.scene = two_cubes(cube, checker);

    // Вторая сцена ждёт своей очереди прямо здесь. Хранить её больше негде —
    // и не нужно: движок про неё знать не обязан
    let mut spare = sphere_and_pyramid(sphere, pyramid, checker);

    println!("S — сменить сцену, Escape — выход");

    // Клавиша считается нажатой, пока её держат, поэтому голая проверка
    // сработала бы каждый кадр и сцены замелькали бы с частотой FPS. Нужен
    // ФРОНТ нажатия: сработать один раз в момент, когда клавиша была отпущена,
    // а стала зажата
    let mut was_pressed = false;
    let mut angle: f32 = 0.0;

    app.set_update(move |scene, pressed_keys, dt| {
        let pressed = pressed_keys.contains(&KeyCode::KeyS);

        if pressed && !was_pressed {
            // Обмен, а не присваивание: Scene не Clone, да и клонировать
            // незачем — обе сцены остаются живыми, просто меняются местами.
            // Ушедшая сохраняет своё состояние и вернётся ровно такой же
            std::mem::swap(scene, &mut spare);
        }

        was_pressed = pressed;

        angle += 45.0 * dt;

        for instance in &mut scene.instances {
            instance.rotation.y = angle;
        }
    });

    event_loop.run_app(&mut app)?;

    Ok(())
}
