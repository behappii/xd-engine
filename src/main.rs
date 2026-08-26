use winit::{event_loop::EventLoop, keyboard::KeyCode};

use crate::{
    app::EngineApp,
    math::Vec3,
    scene::{Instance, Mesh},
};

mod app;
mod math;
mod renderer;
mod scene;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Создание цикла обработки обновлений winit
    let event_loop = EventLoop::new()?;

    // Создаем Движок
    let mut app = EngineApp::new();

    // Генерируем меши
    let cube = Mesh::create_cube();
    let pyramid = Mesh::create_pyramid();

    // Создаем инстансы
    let mut obj1 = Instance::new(cube, Vec3::new(-1.8, 0.0, 0.0));
    obj1.scale = Vec3::new(1.2, 1.2, 1.2);

    let mut obj2 = Instance::new(pyramid, Vec3::new(1.8, 0.0, 0.0));
    obj2.scale = Vec3::new(1.4, 1.4, 1.4);

    // Легко добавляем третий объект (еще один куб повыше), просто написав одну строчку!
    let mut obj3 = Instance::new(Mesh::create_cube(), Vec3::new(0.0, 1.5, -1.0));
    obj3.scale = Vec3::new(0.6, 0.6, 0.6);

    // Закидываем инстансы в сцену движка
    app.scene.add_instance(obj1);
    app.scene.add_instance(obj2);
    app.scene.add_instance(obj3);

    // создаем переменную, которую будем изменять в цикле обновления
    let mut angle: f32 = 0.0;

    // влезаем в цикл обновления и добавляем анимацию кубам
    app.set_update(move |scene, pressed_keys, dt| {
        // АНИМАЦИЯ ИНСТАНСОВ

        angle += 45.0 * dt;

        scene.instances[0].rotation.y = angle;
        scene.instances[0].position.y = (angle * std::f32::consts::PI / 180.0).sin() * 0.5;

        // КАМЕРА

        // Задаем скорость в секунду
        let movement_speed = 4.0 * dt;
        let rotation_speed = 100.0 * dt;

        // Расчет движения камеры на сцене
        let yaw_rad = scene.yaw.to_radians();
        let pitch_rad = scene.pitch.to_radians();

        // перевод сферических координат в вектор для взгляда вперед
        let forward = Vec3::new(
            yaw_rad.cos() * pitch_rad.cos(),
            pitch_rad.sin(),
            yaw_rad.sin() * pitch_rad.cos(),
        )
        .normalize();

        // вектор для просчета движения вбок (под 90 градусов относительно взгляда)
        let right = forward.cross(&Vec3::new(0.0, 1.0, 0.0)).normalize();

        // Проверяем зажатые клавиши через метод .contains() хэш-карты
        if pressed_keys.contains(&KeyCode::KeyW) {
            scene.camera_position = scene.camera_position + forward * movement_speed;
        }
        if pressed_keys.contains(&KeyCode::KeyS) {
            scene.camera_position = scene.camera_position - forward * movement_speed;
        }
        if pressed_keys.contains(&KeyCode::KeyA) {
            scene.camera_position = scene.camera_position - right * movement_speed;
        }
        if pressed_keys.contains(&KeyCode::KeyD) {
            scene.camera_position = scene.camera_position + right * movement_speed;
        }
        if pressed_keys.contains(&KeyCode::KeyH) {
            println!("Hello!");
        }

        // Поворот камеры стрелочками
        if pressed_keys.contains(&KeyCode::ArrowLeft) {
            scene.yaw -= rotation_speed;
        }
        if pressed_keys.contains(&KeyCode::ArrowRight) {
            scene.yaw += rotation_speed;
        }
        if pressed_keys.contains(&KeyCode::ArrowUp) {
            scene.pitch = (scene.pitch + rotation_speed).clamp(-89.0, 89.0);
        }
        if pressed_keys.contains(&KeyCode::ArrowDown) {
            scene.pitch = (scene.pitch - rotation_speed).clamp(-89.0, 89.0);
        }
    });

    // Запускаем приложение
    event_loop.run_app(&mut app)?;

    Ok(())
}
