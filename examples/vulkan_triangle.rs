//! Та же сцена, но через GPU: `cargo run --release --example vulkan_triangle`.
//!
//! Имя файла осталось от Фазы 1, когда здесь и правда был треугольник,
//! зашитый в шейдер. Содержимого оно больше не описывает — но команда запуска
//! успела попасть и в заметки, и в историю разработки, поэтому оставлена.
//!
//! **Смысл примера — в том, чего в нём НЕТ.** Ни одной строчки про Vulkan:
//! ни swapchain, ни дескрипторов, ни командных буферов. Сцена собирается теми
//! же вызовами, что и в `demo.rs`/`minimal.rs` на CPU-пути — `assets.add_mesh`,
//! `Instance::new`, `scene.add_instance`, `set_update`, — и отличается один
//! тип: `VulkanApp` вместо `EngineApp`. Ради этого мост к сцене и делался:
//! игра не знает, каким бэкендом её рисуют.
//!
//! Сравнивать удобнее всего с `demo.rs`: примитивы и процедурная шахматка
//! там те же. Побайтового совпадения не будет — правила растеризации разные,
//! — а вот «на GPU грань светится, на CPU нет» видно мгновенно.
//!
//! Управление: `WASD` — движение, `Space`/`LShift` — вверх/вниз, стрелки —
//! поворот камеры, `Escape` — выход. Всё это живёт в колбэке обновления, как
//! и в CPU-демо, — движок камерой не занимается.
//!
//! Требует установленного Vulkan (Vulkan SDK для macOS/LunarG или
//! `brew install molten-vk`).

// winit берётся через реэкспорт движка, а не своей зависимостью:
// так его версия заведомо совпадает с той, на которой собран xd_engine
use xd_engine::{
    KeyCode,
    config::{CAMERA_MOVEMENT_SPEED, CAMERA_ROTATION_SPEED},
    math::Vec3,
    scene::{Instance, Mesh},
    texture::{Magnify, Minify, Texture},
    vulkan::VulkanApp,
    winit::event_loop::EventLoop,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;

    // Единственная строчка, которой этот пример отличается от CPU-демо
    let mut app = VulkanApp::new();

    // Меши и текстуры отдаются аренам один раз, обратно приходят хендлы —
    // обычные числа, Copy: раздавай скольким угодно инстансам
    let cube = app.assets.add_mesh(Mesh::create_cube());
    let sphere = app.assets.add_mesh(Mesh::create_sphere(16, 24));
    let pyramid = app.assets.add_mesh(Mesh::create_pyramid());

    // Процедурная шахматка — ни файла, ни художника, и пример запускается на
    // свежем клоне без подготовки.
    //
    // `Minify::Linear`, а не `Mipmapped`: мип-пирамиды на GPU-пути пока нет
    // (сэмплер зажат в уровень 0), и просить её значило бы обещать то, чего
    // тут не произойдёт. Отсюда же рябь на полу у горизонта — на CPU-пути её
    // нет, и это самая заметная разница между двумя картинками
    let checker = app.assets.add_texture(
        Texture::checker(64, 8, [230, 230, 230, 255], [40, 40, 60, 255])
            .with_filter(Magnify::Nearest, Minify::Linear),
    );

    // Пол — тот же куб, расплющенный по Y. Он здесь ради перспективной
    // коррекции: клетки уходят к горизонту, и любая ошибка в интерполяции UV
    // сразу выгнет их дугой. UV умножается на 12, чтобы одна картинка
    // разложилась плиткой — за это отвечает режим repeat
    let mut floor_mesh = Mesh::create_cube();
    for vertex in &mut floor_mesh.vertices {
        vertex.uv = vertex.uv * 12.0;
    }

    let mut floor = Instance::new(app.assets.add_mesh(floor_mesh), Vec3::new(0.0, -3.0, 0.0))
        .with_color([255, 255, 255, 255])
        .with_texture(checker);
    floor.scale = Vec3::new(20.0, 0.2, 20.0);
    app.scene.add_instance(floor);

    // Текстурированный куб. Цвет белый не случайно: тексель на него
    // умножается, и любой другой сработал бы как светофильтр поверх картинки
    let mut textured = Instance::new(cube, Vec3::new(-2.0, 0.0, 0.0))
        .with_color([255, 255, 255, 255])
        .with_texture(checker);
    textured.scale = Vec3::new(0.8, 0.8, 0.8);
    app.scene.add_instance(textured);

    // А этот — без текстуры вовсе. На GPU ему достаётся белая заглушка 1×1, и
    // цвет инстанса обязан дойти до экрана неискажённым
    let mut plain = Instance::new(cube, Vec3::new(0.6, 0.0, 0.0)).with_color([230, 120, 40, 255]);
    plain.scale = Vec3::new(0.8, 0.8, 0.8);
    app.scene.add_instance(plain);

    // Сфера — единственный гладкий меш сцены: на ней видно затенение по Гуро,
    // которого на кубе не разглядеть (у плоской грани все три яркости вершин
    // совпадают, интерполировать нечего)
    let mut ball = Instance::new(sphere, Vec3::new(3.0, 0.3, -1.0))
        .with_color([255, 255, 255, 255])
        .with_texture(checker);
    ball.scale = Vec3::new(1.2, 1.2, 1.2);
    app.scene.add_instance(ball);

    // Сплющенная по Y пирамида — тот самый неравномерный масштаб, на котором
    // расходятся матрица нормалей и наивное переиспользование модельной
    let mut squashed =
        Instance::new(pyramid, Vec3::new(-4.5, -1.2, -2.0)).with_color([200, 90, 150, 255]);
    squashed.scale = Vec3::new(1.6, 0.5, 1.6);
    app.scene.add_instance(squashed);

    // Камера — часть сцены, а не движка
    app.scene.camera_position = Vec3::new(0.0, 1.5, 9.0);
    app.scene.yaw = -90.0;

    // Пол крутиться не должен: анимация ниже вращает всё подряд
    let floor_index = 0;

    let mut angle: f32 = 0.0;

    // Колбэк тот же, что у CPU-демо, вплоть до сигнатуры: сцена, зажатые
    // клавиши, дельта времени. Код анимации и управления переносится между
    // бэкендами копированием, без единой правки
    app.set_update(move |scene, pressed_keys, dt| {
        angle += 45.0 * dt;

        for (i, instance) in scene.instances.iter_mut().enumerate() {
            if i == floor_index {
                continue;
            }

            instance.rotation.y = angle + (i as f32);
        }

        // КАМЕРА

        let movement_speed = CAMERA_MOVEMENT_SPEED * dt;
        let rotation_speed = CAMERA_ROTATION_SPEED * dt;

        // Направление взгляда спрашивается у самой сцены: та же формула, что
        // использует и матрица вида на обоих бэкендах
        let forward = scene.forward();
        // Вбок — векторное произведение взгляда и «вверх». Считается от
        // текущего направления, иначе стрейф перестал бы соответствовать
        // повороту камеры
        let right = forward.cross(&Vec3::new(0.0, 1.0, 0.0)).normalize();

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
        if pressed_keys.contains(&KeyCode::Space) {
            scene.camera_position.y += movement_speed;
        }
        if pressed_keys.contains(&KeyCode::ShiftLeft) {
            scene.camera_position.y -= movement_speed;
        }

        if pressed_keys.contains(&KeyCode::ArrowLeft) {
            scene.yaw -= rotation_speed;
        }
        if pressed_keys.contains(&KeyCode::ArrowRight) {
            scene.yaw += rotation_speed;
        }
        // Зажим, а не заворачивание: на 90° взгляд сонаправлен с вектором
        // «вверх», и матрица вида там вырождается
        if pressed_keys.contains(&KeyCode::ArrowUp) {
            scene.pitch = (scene.pitch + rotation_speed).clamp(-89.0, 89.0);
        }
        if pressed_keys.contains(&KeyCode::ArrowDown) {
            scene.pitch = (scene.pitch - rotation_speed).clamp(-89.0, 89.0);
        }
    });

    event_loop.run_app(&mut app)?;

    Ok(())
}
