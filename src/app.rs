use crate::config::{CLEAR_COLOR, HEIGHT, RENDER_SCALE, WIDTH, WINDOW_TITLE};
use crate::renderer::clear_frame;
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
    // Scene - для вызова инстансов или еще чего-то что лежит в сцене
    // HashSet<KeyCode> - для обработки нажатия клавиш (press)
    // f32 - взять dt для независимости обработки кадров от FPS
    update_callback: Option<Box<dyn FnMut(&mut Scene, &HashSet<KeyCode>, f32)>>,
    depth_buffer: Vec<f32>,

    // Размер буфера кадра. Не константа и не размер окна: окно можно растянуть,
    // а буфер вдобавок может быть мельче окна (см. RENDER_SCALE). Всё, что
    // завязано на размер — depth-буфер, индексация пикселей, aspect матрицы
    // проекции — считается от этой пары, поэтому она единственный источник правды
    frame_width: u32,
    frame_height: u32,

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
            depth_buffer: Vec::new(),
            // Пока окна нет, буфера тоже нет. Настоящие размеры проставит
            // `resumed`, когда ОС наконец выдаст окно и его можно будет спросить
            frame_width: 0,
            frame_height: 0,
            fps_counter: FpsCounter::new(),
        }
    }

    pub fn set_update<F>(&mut self, callback: F)
    where
        F: FnMut(&mut Scene, &HashSet<KeyCode>, f32) + 'static,
    {
        self.update_callback = Some(Box::new(callback));
    }

    /// Подгоняет под новый размер окна всё, что от него зависит: поверхность
    /// вывода, буфер кадра и depth-буфер.
    ///
    /// Один и тот же путь для создания окна и для его растягивания — иначе
    /// два места неизбежно разъезжаются, и какой-нибудь буфер остаётся
    /// прежнего размера. Aspect отдельно чинить не нужно: `Scene::draw`
    /// считает его из переданных размеров кадра, а не из констант
    fn resize(&mut self, physical_width: u32, physical_height: u32) {
        // Свёрнутое окно приходит нулевым размером, а текстура нулевой ширины —
        // ошибка в wgpu. Ничего не трогаем: буферы остаются прежними и валидными,
        // а разворачивание окна пришлёт нормальный Resized
        if physical_width == 0 || physical_height == 0 {
            return;
        }

        let Some(pixels) = self.pixels.as_mut() else {
            return;
        };

        let (frame_width, frame_height) = frame_size(physical_width, physical_height);

        // Порядок важен: поверхность — всегда в физических пикселях окна,
        // и знать её размер надо ДО пересборки буфера, потому что от их
        // отношения зависит матрица растяжения внутри `pixels`
        if let Err(err) = pixels.resize_surface(physical_width, physical_height) {
            println!("Не удалось изменить размер поверхности: {err}");
            return;
        }

        if let Err(err) = pixels.resize_buffer(frame_width, frame_height) {
            println!("Не удалось изменить размер буфера кадра: {err}");
            return;
        }

        // Глубина хранится по пикселю, значит её буфер обязан идти в ногу
        // с кадром. Что окажется в новых ячейках — неважно: кадр всё равно
        // начинается с fill(0.0)
        self.depth_buffer
            .resize((frame_width * frame_height) as usize, 0.0);

        self.frame_width = frame_width;
        self.frame_height = frame_height;
    }
}

/// Размер буфера кадра для окна заданного физического размера.
///
/// Вынесено из `resize`, потому что то же самое нужно и при создании окна:
/// `Pixels::new` хочет размер буфера сразу, до того как `resize` вообще
/// сможет что-то поменять
fn frame_size(physical_width: u32, physical_height: u32) -> (u32, u32) {
    (
        ((physical_width as f32 * RENDER_SCALE) as u32).max(1),
        ((physical_height as f32 * RENDER_SCALE) as u32).max(1),
    )
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

        // WIDTH/HEIGHT — это лишь то, что мы ПОПРОСИЛИ у ОС, и в логических
        // пикселях. Сколько вышло на самом деле, знает только окно: ОС могла
        // не дать запрошенный размер, а на Retina масштаб ещё и не единица.
        // Раньше сюда шли константы, и на экране с масштабом 2 поверхность
        // считалась вдвое меньше настоящей — картинку растягивал компоновщик,
        // отсюда и мыло
        let size = window.inner_size();
        // Свежесозданное окно нулевым не бывает, но `Pixels::new` на нулевом
        // размере вернёт ошибку, а тут .unwrap() — подстрахуемся
        let surface_width = size.width.max(1);
        let surface_height = size.height.max(1);
        let (frame_width, frame_height) = frame_size(surface_width, surface_height);

        // Настраиваем пиксельный буфер pixels поверх созданного окна.
        // Два разных размера: поверхность — физические пиксели окна,
        // буфер — наше внутреннее разрешение
        let surface_texture = SurfaceTexture::new(surface_width, surface_height, window.clone());
        let pixels = Pixels::new(frame_width, frame_height, surface_texture).unwrap();

        self.depth_buffer = vec![0.0; (frame_width * frame_height) as usize];
        self.frame_width = frame_width;
        self.frame_height = frame_height;
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

            // Окно растянули мышкой (или развернули на весь экран).
            //
            // Смена DPI — перенос окна на монитор с другим масштабом — отдельно
            // не ловится намеренно: winit после ScaleFactorChanged всё равно
            // присылает Resized с уже пересчитанным размером
            WindowEvent::Resized(new_size) => {
                self.resize(new_size.width, new_size.height);
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
                clear_frame(frame, CLEAR_COLOR);

                // 0.0 = бесконечно далеко, так как храним 1/w
                self.depth_buffer.fill(0.0);

                // Делегируем отрисовку сцены. Размеры — текущие, а не
                // константы: от них зависит и индексация пикселей,
                // и aspect матрицы проекции внутри draw
                self.scene.draw(
                    frame,
                    &mut self.depth_buffer,
                    self.frame_width,
                    self.frame_height,
                );

                // Выводим буфер на экран окна
                if let Err(err) = pixels.render() {
                    println!("Ошибка рендеринга: {:?}", err);
                    event_loop.exit();
                }

                if self.fps_counter.tick() {
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
