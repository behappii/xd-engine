//! Самый маленький работающий пример: `cargo run --release --example minimal`.
//!
//! Два куба одним мешем, процедурная текстура, вращение — всё, что нужно,
//! чтобы увидеть картинку. Файлов не требует принципиально: текстура здесь
//! считается на лету, поэтому пример запускается на свежем клоне без
//! подготовки. Демо-сцена со всеми возможностями — в `examples/demo.rs`.
//!
//! Этот же код лежит в README. Он тут не для украшения: пример собирается при
//! каждом `cargo build --examples`, так что README не сможет молча разойтись
//! с настоящим API.

use winit::event_loop::EventLoop;
use xd_engine::{
    app::EngineApp,
    math::Vec3,
    scene::{Instance, Mesh},
    texture::Texture,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    let mut app = EngineApp::new();

    // Меш и текстура отдаются сцене один раз. Обратно приходят хендлы —
    // обычные числа, Copy: раздавай их скольким угодно объектам
    let cube = app.scene.add_mesh(Mesh::create_cube());
    let checker = app.scene.add_texture(Texture::checker(
        64,
        8,
        [230, 230, 230, 255],
        [40, 40, 60, 255],
    ));

    // Цвет белый не случайно: тексель на него умножается, и любой другой
    // цвет сработал бы как светофильтр поверх картинки
    let mut textured = Instance::new(cube, Vec3::new(-1.2, 0.0, 0.0))
        .with_color([255, 255, 255, 255])
        .with_texture(checker);
    textured.scale = Vec3::new(0.6, 0.6, 0.6);
    app.scene.add_instance(textured);

    // Второй объект тем же мешем — просто ещё раз тот же cube
    let mut plain = Instance::new(cube, Vec3::new(1.2, 0.0, 0.0)).with_color([120, 190, 255, 255]);
    plain.scale = Vec3::new(0.6, 0.6, 0.6);
    app.scene.add_instance(plain);

    // Замыкание вызывается раз в кадр: сцена, зажатые клавиши, дельта времени
    let mut angle = 0.0f32;
    app.set_update(move |scene, _pressed_keys, dt| {
        angle += 45.0 * dt;

        for instance in &mut scene.instances {
            instance.rotation.y = angle;
        }
    });

    event_loop.run_app(&mut app)?;

    Ok(())
}
