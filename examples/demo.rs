//! Демо-сцена: `cargo run --release --example demo`.
//!
//! Живёт в `examples/`, а не в самом крейте, и это не оформительство. Пример
//! подключает `xd_engine` ровно так же, как это сделал бы чужой проект —
//! через публичный API и ничего кроме. Значит всё, чего примеру не хватит,
//! это дырка в публичном API, и видно её сразу, а не когда движок попробуют
//! взять со стороны.

use winit::{event_loop::EventLoop, keyboard::KeyCode};

use xd_engine::{
    app::EngineApp,
    config::{CAMERA_MOVEMENT_SPEED, CAMERA_ROTATION_SPEED},
    math::Vec3,
    scene::{Instance, Mesh},
    texture::Texture,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Создание цикла обработки обновлений winit
    let event_loop = EventLoop::new()?;

    // Создаем Движок
    let mut app = EngineApp::new();

    // Меши регистрируются в сцене один раз и дальше раздаются инстансам по
    // ссылке. MeshId — обычное число и Copy, поэтому ни клонировать, ни
    // заворачивать во что-либо его не нужно
    let cube = app.scene.add_mesh(Mesh::create_cube());
    let pyramid = app.scene.add_mesh(Mesh::create_pyramid());

    // Создаем инстансы
    let mut obj1 = Instance::new(cube, Vec3::new(-3.0, 0.7, 0.0)).with_color([255, 255, 255, 255]);
    obj1.scale = Vec3::new(0.6, 0.6, 0.6);

    let mut obj2 =
        Instance::new(pyramid, Vec3::new(-1.5, 0.3, 0.0)).with_color([255, 255, 255, 255]);
    obj2.scale = Vec3::new(0.6, 0.6, 0.6);

    // Легко добавляем третий объект (еще один куб повыше), просто написав одну строчку!
    let mut obj3 = Instance::new(cube, Vec3::new(0.0, 0.5, 0.0))
        .with_color([255, 255, 255, 255])
        .as_wireframe();
    obj3.scale = Vec3::new(0.6, 0.6, 0.6);

    // Куб с разноцветными гранями: по 2 треугольника на грань,
    // порядок как в create_cube — back, front, bottom, top, left, right
    let mut obj4 = Instance::new(
        app.scene.add_mesh(Mesh::create_cube()),
        Vec3::new(0.0, 1.5, -1.0),
    )
    .with_face_colors(vec![
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
        let mut cube_ring =
            Instance::new(cube, Vec3::new(angle.cos() * 6.0, -2.0, angle.sin() * 6.0))
                .with_color([200, 200, 60, 255]);
        cube_ring.scale = Vec3::new(0.3, 0.3, 0.3);
        app.scene.add_instance(cube_ring);
    }

    // Закидываем инстансы в сцену движка
    // Сфера — единственный здесь меш с гладким затенением. На кубе и пирамиде
    // разницы не увидеть: у них нормали всех трёх вершин грани совпадают, и
    // интерполировать между ними нечего. Здесь же нормаль непрерывна, и
    // затенение по Гуро размазывает свет по поверхности, скрывая грани
    //
    // Шахматка на ней — не для красоты, а чтобы развёртку было видно: клетки
    // не должны рваться на меридиане 0° (шов сошёлся) и обязаны сбегаться
    // к полюсам клиньями. Ни того ни другого в проволочном режиме не разглядеть.
    // Клеток больше, чем у пола: на 24 долях пара клеток слилась бы в полосы
    let globe = app.scene.add_texture(Texture::checker(
        64,
        8,
        [230, 230, 230, 255],
        [40, 40, 60, 255],
    ));
    let sphere_mesh = app.scene.add_mesh(Mesh::create_sphere(16, 24));

    let mut obj5 = Instance::new(sphere_mesh, Vec3::new(1.8, 0.7, 0.0))
        .with_color([255, 255, 255, 255])
        .with_texture(globe);
    obj5.scale = Vec3::new(0.7, 0.7, 0.7);

    // Процедурная шахматка — не нужен ни файл, ни художник
    let checker_image = Texture::checker(4, 2, [230, 230, 230, 255], [40, 40, 60, 255]);

    // Картинка из файла. Путь относительный: cargo запускает пример из корня
    // проекта, так что отсчёт идёт оттуда.
    // Демо не должно падать из-за отсутствующей картинки: textures/ не в
    // репозитории, и у свежего клона там пусто. Но и молчать нельзя — иначе
    // на кубе окажется шахматка без единого намёка почему, и искать причину
    // будешь в развёртке или в выборке текселя. Отсюда предупреждение
    let photo_image = Texture::load("textures/noname.png").unwrap_or_else(|err| {
        eprintln!("текстура не загрузилась ({err}), берём шахматку");
        // Замыкание только одалживает картинку — сама она уедет в пол ниже
        checker_image.clone()
    });

    let photo = app.scene.add_texture(photo_image);
    let checker = app.scene.add_texture(checker_image);

    // Куб с текстурой. Развёртка `create_cube` отдаёт каждой грани весь
    // квадрат картинки, так что она ложится на грань целиком.
    // Цвет белый не случайно: тексель на него умножается, и любой другой
    // цвет подкрасил бы фотографию
    let mut obj6 = Instance::new(cube, Vec3::new(3.6, 0.7, 0.0))
        .with_color([255, 255, 255, 255])
        .with_texture(photo);
    obj6.scale = Vec3::new(0.6, 0.6, 0.6);

    // Пол — тот же куб, расплющенный по Y. Он здесь ради перспективной
    // коррекции: клетки уходят к горизонту, и любая ошибка в интерполяции
    // UV сразу выгнет их дугой. UV умножается на 12, чтобы одна картинка
    // разложилась плиткой — за это отвечает режим repeat в Texture::sample
    let mut floor_mesh = Mesh::create_cube();
    for vertex in &mut floor_mesh.vertices {
        vertex.uv = vertex.uv * 12.0;
    }

    let mut floor = Instance::new(app.scene.add_mesh(floor_mesh), Vec3::new(0.0, -3.0, 0.0))
        .with_color([255, 255, 255, 255])
        .with_texture(checker);
    floor.scale = Vec3::new(20.0, 0.2, 20.0);

    app.scene.add_instance(obj1);
    app.scene.add_instance(obj2);
    app.scene.add_instance(obj3);
    app.scene.add_instance(obj4);
    app.scene.add_instance(obj5);
    app.scene.add_instance(obj6);
    app.scene.add_instance(floor);

    // Пол крутиться не должен: анимация ниже вращает всё подряд
    let floor_index = app.scene.instances.len() - 1;

    // Ручная деформация ОДНОГО объекта. Куб общий, поэтому сначала кладём в
    // сцену его копию и переводим инстанс на неё, иначе правка разъехалась бы
    // по всем кубам разом. Раньше то же самое делал Rc::make_mut, только
    // молча — теперь копия видна в коде
    let deformed = app.scene.mesh(cube).clone();
    let deformed = app.scene.add_mesh(deformed);
    app.scene.instances[0].mesh = deformed;

    let mesh = app.scene.mesh_mut(deformed);
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
            if i == floor_index {
                continue;
            }

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
