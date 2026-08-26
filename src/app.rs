use crate::config::{CLEAR_COLOR, HEIGHT, WIDTH, WINDOW_TITLE};
use crate::{fps_counter::FpsCounter, scene::Scene};
use pixels::{Pixels, SurfaceTexture};
use std::collections::HashSet;
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

    // публичная сцена, которую можно настраивать снаружи
    pub scene: Scene,
    last_time: Instant,
    pressed_keys: HashSet<KeyCode>, // нажатые клавиши

    // функция для вызова обновлений
    // Scene - для вызова инстансов или еще чего-то что ледит в сцене
    // HashSet<KeyCode> - для обработки нажатия клавиш (press)
    // f32 - взять dt для независимости обработки кадров от FPS
    update_callback: Option<Box<dyn FnMut(&mut Scene, &HashSet<KeyCode>, f32)>>,

    fps_counter: FpsCounter,
}

impl EngineApp {
    pub fn new() -> Self {
        // создание пустой сцены
        Self {
            window: None,
            pixels: None,
            last_time: Instant::now(),
            scene: Scene::new(),
            pressed_keys: HashSet::new(),
            update_callback: None,
            fps_counter: FpsCounter::new(),
        }
    }

    pub fn set_update<F>(&mut self, callback: F)
    where
        F: FnMut(&mut Scene, &HashSet<KeyCode>, f32) + 'static,
    {
        self.update_callback = Some(Box::new(callback));
    }
}

// Реализуем обязательный обработчик событий winit
impl ApplicationHandler for EngineApp {
    // Это событие срабатывает, когда ОС готова дать нам окно
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window_attributes = Window::default_attributes()
            .with_title(WINDOW_TITLE)
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

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    // Обработка системных событий окна (закрытие, перерисовка)
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            // Завершение работы
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            // Cчитывание нажатий клавиш
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(key) = event.physical_key {
                    if event.state.is_pressed() {
                        self.pressed_keys.insert(key); // Зажали — сохраняем
                    } else {
                        self.pressed_keys.remove(&key); // Отжали — стираем
                    }

                    // Быстрый выход из игры по кнопке Escape на системном уровне
                    if key == KeyCode::Escape {
                        event_loop.exit();
                    }
                }
            }

            // Запрос на перерисовку кадра
            WindowEvent::RedrawRequested => {
                let pixels = self.pixels.as_mut().unwrap();
                let window = self.window.as_ref().unwrap();

                // Независимость от счетчика FPS

                // Берем точное время на текущем такте процессора
                let now = Instant::now();
                // Считаем дельту времени
                let dt = now.duration_since(self.last_time).as_secs_f32();
                // Сохраняем текущее время как прошлое чтобы сравнить в следующем кадре сколько прошло времени
                self.last_time = now;

                if let Some(ref mut update) = self.update_callback {
                    update(&mut self.scene, &self.pressed_keys, dt);
                }

                // РЕНДЕРИНГ

                // Достаем буфер экрана
                let frame = pixels.frame_mut();

                // Заливка фона кадра
                for pixel in frame.chunks_exact_mut(4) {
                    pixel[0] = CLEAR_COLOR[0];
                    pixel[1] = CLEAR_COLOR[1];
                    pixel[2] = CLEAR_COLOR[2];
                    pixel[3] = CLEAR_COLOR[3];
                }

                // Делегируем отрисовку сцены
                self.scene.draw(frame);

                // Выводим буфер на экран окна
                if let Err(err) = pixels.render() {
                    println!("Ошибка рендеринга: {:?}", err);
                    event_loop.exit();
                }

                if self.fps_counter.tick() {
                    println!("FPS = {}", self.fps_counter.fps());
                    window.set_title(&format!(
                        "{} | FPS: {}",
                        WINDOW_TITLE,
                        self.fps_counter.fps()
                    ));
                }
            }
            _ => {}
        }
    }
}
