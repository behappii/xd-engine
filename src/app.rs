use crate::{
    math::{Mat4, Vec3},
    renderer::{HEIGHT, WIDTH, draw_line, project},
    scene::{Instance, Mesh},
};
use pixels::{Pixels, SurfaceTexture};
use std::{sync::Arc, time::Instant};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::ActiveEventLoop,
    window::{Window, WindowId},
};

pub struct EngineApp {
    window: Option<Arc<Window>>,
    pixels: Option<Pixels<'static>>,
    angle: f32,
    last_time: Instant,
    instances: Vec<Instance>, // инстансы

    // флаги зажатых клавиш для перемещения
    key_w: bool,
    key_s: bool,
    key_a: bool,
    key_d: bool,
    key_left: bool,
    key_right: bool,
    key_up: bool,
    key_down: bool,
}

impl Default for EngineApp {
    fn default() -> Self {
        let cube = Mesh::create_cube();
        let pyramid = Mesh::create_pyramid();

        let mut instances = vec![
            Instance::new(pyramid.clone(), Vec3::new(0.0, 0.0, 0.0)),
            Instance::new(cube.clone(), Vec3::new(-2.0, 0.0, -2.0)),
            Instance::new(cube.clone(), Vec3::new(0.0, -3.0, 0.0)),
        ];

        instances[0].scale = Vec3::new(0.6, 0.6, 0.6);
        instances[2].scale = Vec3::new(1.2, 0.8, 1.2);

        Self {
            window: None,
            pixels: None,
            angle: 0.0,
            last_time: Instant::now(),
            instances, // инстансы

            // камера
            camera_pos: Vec3::new(0.0, 3.0, 7.0),
            yaw: -90.0,
            pitch: 0.0,

            // Изначально все кнопки отпущены
            key_w: false,
            key_s: false,
            key_a: false,
            key_d: false,
            key_left: false,
            key_right: false,
            key_up: false,
            key_down: false,
        }
    }
}

// Реализуем обязательный обработчик событий winit
impl ApplicationHandler for EngineApp {
    // 1. Это событие срабатывает, когда ОС готова дать нам окно
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window_attributes = Window::default_attributes()
            .with_title("дадада бурмалда")
            .with_inner_size(LogicalSize::new(WIDTH, HEIGHT));

        let raw_window = event_loop.create_window(window_attributes).unwrap();
        let window = Arc::new(raw_window);

        // Настраиваем пиксельный буфер pixels поверх созданного окна
        let surface_texture = SurfaceTexture::new(WIDTH, HEIGHT, window.clone());
        let pixels = Pixels::new(WIDTH, HEIGHT, surface_texture).unwrap();

        self.window = Some(window);
        self.pixels = Some(pixels);
        self.last_time = Instant::now();
    }

    // 2. Обработка системных событий окна (закрытие, перерисовка)
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            // Cчитывание нажатий клавиш
            WindowEvent::KeyboardInput { event, .. } => {
                let is_pressed = event.state.is_pressed();
                if let PhysicalKey::Code(key) = event.physical_key {
                    match key {
                        // вперед/назад/влево/вправо
                        KeyCode::KeyW => self.key_w = is_pressed,
                        KeyCode::KeyS => self.key_s = is_pressed,
                        KeyCode::KeyA => self.key_a = is_pressed,
                        KeyCode::KeyD => self.key_d = is_pressed,

                        // повороты камеры
                        KeyCode::ArrowLeft => self.key_left = is_pressed,
                        KeyCode::ArrowRight => self.key_right = is_pressed,
                        KeyCode::ArrowUp => self.key_up = is_pressed,
                        KeyCode::ArrowDown => self.key_down = is_pressed,

                        // Выход по Escape
                        KeyCode::Escape => event_loop.exit(),
                        _ => {
                            println!("{}", is_pressed);
                        }
                    }
                }
            }

            // Запрос на перерисовку кадра
            WindowEvent::RedrawRequested => {
                let pixels = self.pixels.as_mut().unwrap();
                let window = self.window.as_ref().unwrap();

                // Считаем дельту времени
                let now = Instant::now();
                let dt = now.duration_since(self.last_time).as_secs_f32();
                self.last_time = now;

                self.angle += 45.0 * dt; // скорость вращения

                self.instances[0].rotation = Vec3::new(0.0, self.angle, 0.0);
                self.instances[1].rotation = Vec3::new(15.0, self.angle * 2.0, 20.0);
                self.instances[2].rotation = Vec3::new(30.0, self.angle, 45.0);

                // Достаем буфер экрана
                let frame = pixels.frame_mut();

                // Заливаем фон темно-серым цветом
                for pixel in frame.chunks_exact_mut(4) {
                    pixel[0] = 25; // R
                    pixel[1] = 25; // G
                    pixel[2] = 25; // B
                    pixel[3] = 255; // A
                }

                // Задаем скорость в секунду
                let movement_speed = 4.0 * dt;
                let rotation_speed = 100.0 * dt;

                // Проверяем зажатые клавиши и плавно меняем координаты
                if self.key_w {
                    self.camera_pos = self.camera_pos
                        + Vec3::new(
                            forward.x * movement_speed,
                            forward.y * movement_speed,
                            forward.z * movement_speed,
                        );
                }
                if self.key_s {
                    self.camera_pos = self.camera_pos
                        - Vec3::new(
                            forward.x * movement_speed,
                            forward.y * movement_speed,
                            forward.z * movement_speed,
                        );
                }
                if self.key_a {
                    self.camera_pos = self.camera_pos
                        - Vec3::new(
                            right.x * movement_speed,
                            right.y * movement_speed,
                            right.z * movement_speed,
                        );
                }
                if self.key_d {
                    self.camera_pos = self.camera_pos
                        + Vec3::new(
                            right.x * movement_speed,
                            right.y * movement_speed,
                            right.z * movement_speed,
                        );
                }

                // Поворот камеры стрелочками
                if self.key_left {
                    self.yaw -= rotation_speed;
                }
                if self.key_right {
                    self.yaw += rotation_speed;
                }
                if self.key_up {
                    self.pitch = (self.pitch + rotation_speed).clamp(-89.0, 89.0);
                }
                if self.key_down {
                    self.pitch = (self.pitch - rotation_speed).clamp(-89.0, 89.0);
                }

                // Выводим буфер на экран окна
                if let Err(err) = pixels.render() {
                    println!("Ошибка рендеринга: {:?}", err);
                    event_loop.exit();
                }

                // Сразу запрашиваем следующий кадр для бесконечной анимации
                window.request_redraw();
            }
            _ => {}
        }
    }
}
