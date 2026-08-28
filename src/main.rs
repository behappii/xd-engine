use std::rc::Rc;

use winit::{event_loop::EventLoop, keyboard::KeyCode};

// Движок живёт в библиотечном крейте (src/lib.rs), а этот файл — обычный
// внешний пользователь: он собирает сцену и запускает цикл.
use xd_engine::{
    app::EngineApp,
    config::{CAMERA_MOVEMENT_SPEED, CAMERA_ROTATION_SPEED},
    math::Vec3,
    scene::{Instance, Mesh},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Создание цикла обработки обновлений winit
    let event_loop = EventLoop::new()?;

    // Создаем Движок
    let mut app = EngineApp::new();

    // Генерируем меши
    let cube = Rc::new(Mesh::create_cube());
    let pyramid = Rc::new(Mesh::create_pyramid());

    // Создаем инстансы
    let mut obj1 =
        Instance::new(Rc::clone(&cube), Vec3::new(-3.0, 0.7, 0.0)).with_color([255, 255, 255, 255]);
    obj1.scale = Vec3::new(0.6, 0.6, 0.6);

    let mut obj2 =
        Instance::new(pyramid, Vec3::new(-1.5, 0.3, 0.0)).with_color([255, 255, 255, 255]);
    obj2.scale = Vec3::new(0.6, 0.6, 0.6);

    // Легко добавляем третий объект (еще один куб повыше), просто написав одну строчку!
    let mut obj3 = Instance::new(Rc::clone(&cube), Vec3::new(0.0, 0.5, 0.0))
        .with_color([255, 255, 255, 255])
        .as_wireframe();
    obj3.scale = Vec3::new(0.6, 0.6, 0.6);

    // Куб с разноцветными гранями: по 2 треугольника на грань,
    // порядок как в create_cube — back, front, bottom, top, left, right
    let mut obj4 =
        Instance::new(Mesh::create_cube(), Vec3::new(0.0, 1.5, -1.0)).with_face_colors(vec![
            [255, 80, 80, 255],
            [255, 80, 80, 255], // back
            [80, 255, 80, 255],
            [80, 255, 80, 255], // front
            [80, 80, 255, 255],
            [80, 80, 255, 255], // bottom
            [255, 255, 80, 255],
            [255, 255, 80, 255], // top
            [255, 80, 255, 255],
            [255, 80, 255, 255], // left
            [80, 255, 255, 255],
            [80, 255, 255, 255], // right
        ]);
    obj4.scale = Vec3::new(0.6, 0.6, 0.6);

    for i in 0..20 {
        let angle = i as f32 * 18.0_f32.to_radians();
        let mut cube_ring = Instance::new(
            Rc::clone(&cube),
            Vec3::new(angle.cos() * 6.0, -2.0, angle.sin() * 6.0),
        )
        .with_color([200, 200, 60, 255]);
        cube_ring.scale = Vec3::new(0.3, 0.3, 0.3);
        app.scene.add_instance(cube_ring);
    }

    // Закидываем инстансы в сцену движка
    // Сфера — единственный здесь меш с гладким затенением. На кубе и пирамиде
    // разницы не увидеть: у них нормали всех трёх вершин грани совпадают, и
    // интерполировать между ними нечего. Здесь же нормаль непрерывна, и
    // затенение по Гуро размазывает свет по поверхности, скрывая грани
    let mut obj5 = Instance::new(Mesh::create_sphere(16, 24), Vec3::new(1.8, 0.7, 0.0))
        .with_color([120, 190, 255, 255])
        .as_wireframe();
    obj5.scale = Vec3::new(0.7, 0.7, 0.7);

    app.scene.add_instance(obj1);
    app.scene.add_instance(obj2);
    app.scene.add_instance(obj3);
    app.scene.add_instance(obj4);
    app.scene.add_instance(obj5);

    // Ручная деформация одного экземпляра меша.
    // Rc::make_mut видит, что куб разделяют несколько инстансов, и молча
    // клонирует его — правки достанутся только instances[0]
    let mesh = Rc::make_mut(&mut app.scene.instances[0].mesh);
    mesh.vertices[0].position.y -= 1.0;
    mesh.vertices[1].position.x += 2.0;
    mesh.vertices[5].position.z += 4.0;
    // Позиции сдвинулись — предвычисленные нормали устарели и свет на этих
    // гранях врал бы. Пересчитываем.
    // Учти: у меша с плоским затенением вершины расщеплены, поэтому соседние
    // грани больше не делят вершину и деформация рвёт куб по рёбрам
    mesh.recalculate_flat_normals();

    // создаем переменную, которую будем изменять в цикле обновления
    let mut angle: f32 = 0.0;

    // влезаем в цикл обновления и добавляем анимацию кубам
    app.set_update(move |scene, pressed_keys, dt| {
        // АНИМАЦИЯ ИНСТАНСОВ

        angle += 45.0 * dt;

        // Итерируемся по всем инстансам, а не по жёстко зашитому числу:
        // теперь добавление объекта в сцену не ломает анимацию
        for (i, instance) in scene.instances.iter_mut().enumerate() {
            instance.rotation.y = angle + (i as f32);
            // instance.position.y = (angle * std::f32::consts::PI / 180.0).sin() * ((i + 2) as f32);
        }

        // КАМЕРА

        // Задаем скорость в секунду
        let movement_speed = CAMERA_MOVEMENT_SPEED * dt;
        let rotation_speed = CAMERA_ROTATION_SPEED * dt;

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
        // Пробел — летим строго вверх по оси Y
        if pressed_keys.contains(&KeyCode::Space) {
            scene.camera_position.y += movement_speed;
        }
        // Левый Shift — летим строго вниз по оси Y
        if pressed_keys.contains(&KeyCode::ShiftLeft) {
            scene.camera_position.y -= movement_speed;
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
