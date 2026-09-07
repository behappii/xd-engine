//! GPU-бэкенд, рисующий НАСТОЯЩУЮ сцену — тот же `Scene` + `Assets`, что и
//! CPU-растеризатор.
//!
//! Имя файла осталось от Фазы 1, когда здесь и правда был один треугольник,
//! зашитый в шейдер. Сегодня оно уже не описывает содержимое — но и менять
//! его отдельным движением незачем: `cargo run --example vulkan_triangle`
//! успел попасть и в заметки, и в историю разработки.
//!
//! Пример нарочно НЕ использует `EngineApp`: тот завязан на `pixels` и на
//! CPU-буфер кадра, а здесь картинку показывает swapchain. Своё окно и свой
//! `ApplicationHandler` — зато CPU-путь не тронут ни строкой, как и было
//! условлено с самого начала.
//!
//! Сцена собрана так, чтобы её было с чем сравнивать: те же примитивы и та
//! же процедурная шахматка, что в `examples/demo.rs`. Открыв оба примера
//! рядом, расхождения между двумя бэкендами видно сразу — побайтового
//! совпадения не будет (правила растеризации разные), а вот «на GPU грань
//! светится, на CPU нет» заметно мгновенно.
//!
//! Управление: `WASD` — движение, `Space`/`LShift` — вверх/вниз, стрелки —
//! поворот камеры, `Escape` — выход. То же, что и у CPU-демо.
//!
//! Требует установленного Vulkan (Vulkan SDK для macOS/LunarG или
//! `brew install molten-vk` — на Mac этот путь идёт через MoltenVK, см.
//! doc-комментарий `xd_engine::vulkan`).

use std::collections::HashSet;
use std::time::Instant;

use xd_engine::math::Vec3;
use xd_engine::scene::{Assets, Instance, Mesh, Scene};
use xd_engine::texture::{Magnify, Minify, Texture};
use xd_engine::vulkan::VulkanRenderer;
use xd_engine::winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

/// Скорость движения камеры в единицах мира за секунду
const MOVE_SPEED: f32 = 6.0;
/// Скорость поворота камеры в градусах за секунду
const LOOK_SPEED: f32 = 90.0;

struct VulkanApp {
    window: Option<Window>,
    renderer: Option<VulkanRenderer>,
    scene: Scene,
    assets: Assets,
    pressed_keys: HashSet<KeyCode>,
    last_frame: Instant,
}

impl VulkanApp {
    fn new() -> Self {
        let mut assets = Assets::new();
        let scene = build_scene(&mut assets);

        Self {
            window: None,
            renderer: None,
            scene,
            assets,
            pressed_keys: HashSet::new(),
            last_frame: Instant::now(),
        }
    }

    /// Движение камеры за прошедшее время.
    ///
    /// `dt`, а не «столько-то за кадр»: иначе скорость перемещения зависела
    /// бы от FPS, и на быстрой машине камера летела бы вдвое быстрее. Тот же
    /// принцип, что у `update`-колбэка `EngineApp` на CPU-пути
    fn update_camera(&mut self, dt: f32) {
        let held = |key| self.pressed_keys.contains(&key);

        if held(KeyCode::ArrowLeft) {
            self.scene.yaw -= LOOK_SPEED * dt;
        }
        if held(KeyCode::ArrowRight) {
            self.scene.yaw += LOOK_SPEED * dt;
        }
        if held(KeyCode::ArrowUp) {
            self.scene.pitch += LOOK_SPEED * dt;
        }
        if held(KeyCode::ArrowDown) {
            self.scene.pitch -= LOOK_SPEED * dt;
        }
        // Зажим, а не заворачивание: на 90° взгляд смотрит строго вверх, и
        // `look_at` там вырождается — направление становится сонаправлено с
        // вектором «вверх», и матрица вида перестаёт быть определена
        self.scene.pitch = self.scene.pitch.clamp(-89.0, 89.0);

        let forward = self.scene.forward();
        // Вправо — векторное произведение направления и «вверх». Считается от
        // текущего взгляда, а не берётся константой, иначе стрейф перестал бы
        // соответствовать повороту камеры
        let right = forward.cross(&Vec3::new(0.0, 1.0, 0.0)).normalize();

        let mut movement = Vec3::new(0.0, 0.0, 0.0);
        if held(KeyCode::KeyW) {
            movement = movement + forward;
        }
        if held(KeyCode::KeyS) {
            movement = movement - forward;
        }
        if held(KeyCode::KeyD) {
            movement = movement + right;
        }
        if held(KeyCode::KeyA) {
            movement = movement - right;
        }
        if held(KeyCode::Space) {
            movement = movement + Vec3::new(0.0, 1.0, 0.0);
        }
        if held(KeyCode::ShiftLeft) {
            movement = movement - Vec3::new(0.0, 1.0, 0.0);
        }

        // Нормировка нужна, чтобы «вперёд и вбок» одновременно не давало
        // скорость в √2 раза больше — классическая диагональная надбавка
        if movement.length() > 0.0 {
            self.scene.camera_position =
                self.scene.camera_position + movement.normalize() * (MOVE_SPEED * dt);
        }
    }
}

impl ApplicationHandler for VulkanApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window_attributes = Window::default_attributes()
            .with_title("xd_engine — сцена через Vulkan")
            .with_inner_size(LogicalSize::new(1024.0, 768.0));
        let window = event_loop.create_window(window_attributes).unwrap();

        match VulkanRenderer::new(&window) {
            Ok(renderer) => self.renderer = Some(renderer),
            Err(err) => {
                eprintln!("не удалось создать Vulkan-рендерер: {err}");
                eprintln!("установлен ли Vulkan SDK / molten-vk (brew install molten-vk)?");
                event_loop.exit();
                return;
            }
        }

        self.window = Some(window);
        self.last_frame = Instant::now();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::KeyboardInput { event, .. } => {
                if event.physical_key == PhysicalKey::Code(KeyCode::Escape) {
                    event_loop.exit();
                    return;
                }
                if let PhysicalKey::Code(code) = event.physical_key {
                    match event.state {
                        ElementState::Pressed => {
                            self.pressed_keys.insert(code);
                        }
                        ElementState::Released => {
                            self.pressed_keys.remove(&code);
                        }
                    }
                }
            }

            // Физические пиксели, а не логические: поверхность вывода всегда
            // физическая, и на Retina это вдвое больше по каждой оси.
            // Свёрнутое окно на Windows приходит сюда нулевым размером —
            // рендерер это переживает сам, пропуская кадры
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size.width, size.height);
                }
            }

            // Потеря фокуса отпускает все клавиши — иначе Alt-Tab с зажатым
            // `W` оставит камеру ехать самой по себе навсегда: отпускание
            // достанется уже другому окну (CLAUDE.md, «Потеря фокуса
            // отпускает все клавиши»)
            WindowEvent::Focused(false) => self.pressed_keys.clear(),

            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = (now - self.last_frame).as_secs_f32();
                self.last_frame = now;

                self.update_camera(dt);

                if let Some(renderer) = self.renderer.as_mut() {
                    if let Err(err) = renderer.draw(&self.scene, &self.assets) {
                        eprintln!("ошибка рендеринга: {err}");
                        event_loop.exit();
                    }
                }
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

/// Сцена строится ровно теми же вызовами, что и на CPU-пути — `Assets`,
/// `Mesh`, `Instance`, `Scene`. Ни одной строчки про Vulkan здесь нет, и это
/// главное, ради чего затевался мост: игра не знает, каким бэкендом её
/// рисуют
fn build_scene(assets: &mut Assets) -> Scene {
    // Шахматка процедурная, а не из файла: примеру не нужен ни путь к
    // ассетам, ни картинка в репозитории. Развёртку куба и шов сферы на ней
    // видно сразу
    let checker = assets.add_texture(
        Texture::checker(8, 4, [230, 230, 230, 255], [40, 40, 60, 255])
            .with_filter(Magnify::Nearest, Minify::Linear),
    );

    let cube = assets.add_mesh(Mesh::create_cube());
    let sphere = assets.add_mesh(Mesh::create_sphere(16, 24));
    let pyramid = assets.add_mesh(Mesh::create_pyramid());

    // Пол — тот же куб, расплющенный по Y, с UV, умноженным на 12: одна
    // картинка раскладывается плиткой (за это отвечает режим repeat). Он
    // здесь ровно затем же, зачем в CPU-демо, — уходящие к горизонту клетки
    // мгновенно показывают любую ошибку в перспективной коррекции
    let mut floor_mesh = Mesh::create_cube();
    for vertex in &mut floor_mesh.vertices {
        vertex.uv = vertex.uv * 12.0;
    }
    let floor_mesh = assets.add_mesh(floor_mesh);

    let mut scene = Scene::new();
    scene.camera_position = Vec3::new(0.0, 1.5, 9.0);
    // -90° — взгляд вдоль минус Z, то есть на сцену, стоящую в начале
    // координат (см. `Scene::forward`)
    scene.yaw = -90.0;

    let mut floor = Instance::new(floor_mesh, Vec3::new(0.0, -3.0, 0.0))
        .with_color([255, 255, 255, 255])
        .with_texture(checker);
    floor.scale = Vec3::new(20.0, 0.2, 20.0);
    scene.add_instance(floor);

    // Текстурированный куб — проверка того, что UV и дескриптор доезжают до
    // нужного инстанса, а не берутся от предыдущего
    let mut textured = Instance::new(cube, Vec3::new(-2.0, 0.0, 0.0))
        .with_color([255, 255, 255, 255])
        .with_texture(checker);
    textured.rotation = Vec3::new(0.0, 25.0, 0.0);
    scene.add_instance(textured);

    // А этот — без текстуры вовсе: ему достаётся белая заглушка 1x1, и цвет
    // инстанса обязан дойти до экрана неискажённым (белый тексель —
    // тождественная операция для покомпонентного умножения)
    scene.add_instance(
        Instance::new(cube, Vec3::new(0.8, 0.0, 0.0)).with_color([230, 120, 40, 255]),
    );

    // Сфера — единственный гладкий меш сцены: на ней видно затенение по
    // Гуро, которого на кубе не разглядеть (у плоских граней все три яркости
    // вершины совпадают)
    let mut ball = Instance::new(sphere, Vec3::new(3.2, 0.3, -1.0)).with_color([90, 170, 230, 255]);
    ball.scale = Vec3::new(1.3, 1.3, 1.3);
    scene.add_instance(ball);

    // Сплющенная по Y пирамида — тот самый неравномерный масштаб, на котором
    // расходятся `normal_matrix()` и наивное переиспользование `model`
    let mut squashed =
        Instance::new(pyramid, Vec3::new(-4.5, -1.2, -2.0)).with_color([200, 90, 150, 255]);
    squashed.scale = Vec3::new(1.6, 0.5, 1.6);
    scene.add_instance(squashed);

    scene
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let mut app = VulkanApp::new();
    event_loop.run_app(&mut app).unwrap();
}
