//! Точка входа GPU-пути — то же, чем для CPU-пути является
//! [`crate::app::EngineApp`]: окно, цикл событий, учёт клавиш, дельта
//! времени и колбэк обновления. Сцена собирается ровно теми же вызовами, что
//! и на CPU (`assets.add_mesh`, `Instance::new`, `scene.add_instance`,
//! `set_update`), меняется только имя типа.
//!
//! **Почему отдельный тип, а не флаг у `EngineApp`.** Развилка обсуждалась, и
//! выбран этот вариант сознательно: `EngineApp` завязан на `pixels` и на
//! CPU-буфер кадра с depth-буфером, а здесь ни того ни другого нет вовсе —
//! кадр показывает swapchain, глубина живёт на видеокарте. Флаг внутри
//! `EngineApp` означал бы ветку в каждом методе и половину полей, не имеющих
//! смысла для второй ветки. Вместо этого CPU-путь остался нетронутым — то же
//! условие, что действовало с первого дня работы над Vulkan.
//!
//! **Чем за это заплачено, честно.** Учёт зажатых клавиш, сброс по потере
//! фокуса, дельта времени и счётчик FPS здесь повторены. Их немного, но это
//! ровно те места, где в `app.rs` уже жили баги (см. CLAUDE.md, «Потеря
//! фокуса отпускает все клавиши»), — поэтому логика повторена ОДИН В ОДИН, с
//! теми же тестами, а не переписана «как удобнее». Если однажды здесь
//! появится третий бэкенд, вот эту обвязку и нужно будет выносить в общее
//! место.
//!
//! Разница с `EngineApp`, которую видно снаружи, ровно одна: у сцены нет
//! `RENDER_SCALE`. Кадр рисуется в размер поверхности, потому что менять его
//! ради скорости у GPU-пути пока незачем.

use std::collections::HashSet;
use std::time::Instant;

use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::ActiveEventLoop,
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

use crate::config::{HEIGHT, WIDTH, WINDOW_TITLE};
use crate::fps_counter::FpsCounter;
use crate::scene::{Assets, Scene};
use crate::vulkan::context::VulkanRenderer;

pub struct VulkanApp {
    window: Option<Window>,
    renderer: Option<VulkanRenderer>,

    /// Мир: что где стоит и откуда на это смотрят. Публичное поле, как и у
    /// `EngineApp` — сцена и есть то, ради чего движок запускают
    pub scene: Scene,

    /// Меши и текстуры. Живут рядом со сценой, а не внутри неё: ресурсы
    /// переживают любую отдельную сцену, и `MeshId` действителен во всех
    pub assets: Assets,

    last_time: Instant,
    pressed_keys: HashSet<KeyCode>,
    update_callback: Option<Box<dyn FnMut(&mut Scene, &HashSet<KeyCode>, f32)>>,
    fps_counter: FpsCounter,
}

impl Default for VulkanApp {
    fn default() -> Self {
        Self::new()
    }
}

impl VulkanApp {
    pub fn new() -> Self {
        Self {
            window: None,
            renderer: None,
            scene: Scene::new(),
            assets: Assets::new(),
            last_time: Instant::now(),
            pressed_keys: HashSet::new(),
            update_callback: None,
            fps_counter: FpsCounter::new(),
        }
    }

    /// Колбэк, вызываемый раз в кадр: сцена, зажатые клавиши, дельта времени.
    ///
    /// Сигнатура совпадает с `EngineApp::set_update` буква в букву, и это не
    /// совпадение: код анимации и управления камерой должен переноситься
    /// между бэкендами копированием, без единой правки
    pub fn set_update<F>(&mut self, callback: F)
    where
        F: FnMut(&mut Scene, &HashSet<KeyCode>, f32) + 'static,
    {
        self.update_callback = Some(Box::new(callback));
    }

    /// Запомнить состояние клавиши.
    ///
    /// Множество ЗАЖАТЫХ клавиш, а не событий: пока клавишу держат, ОС шлёт
    /// повторные нажатия, и в множестве от них ничего не меняется
    fn track_key(&mut self, key: KeyCode, is_pressed: bool) {
        if is_pressed {
            self.pressed_keys.insert(key);
        } else {
            self.pressed_keys.remove(&key);
        }
    }

    /// Окно получило или потеряло фокус.
    ///
    /// Потеря фокуса обязана отпустить ВСЁ: события клавиатуры идут окну в
    /// фокусе, поэтому Alt-Tab с зажатым `W` отдаёт отпускание уже другому
    /// окну, а до нас оно не доедет никогда — клавиша останется зажатой
    /// навсегда, и камера поедет сама по себе.
    ///
    /// Обратное неверно, и перепутанное условие выглядит правдоподобно
    /// («тоже что-то очищает»): возврат фокуса не трогает ничего, множество к
    /// этому моменту и так пустое
    fn focus_changed(&mut self, focused: bool) {
        if !focused {
            self.pressed_keys.clear();
        }
    }
}

impl ApplicationHandler for VulkanApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window_attributes = Window::default_attributes()
            .with_title(WINDOW_TITLE)
            .with_inner_size(LogicalSize::new(WIDTH as f64, HEIGHT as f64));

        let window = match event_loop.create_window(window_attributes) {
            Ok(window) => window,
            Err(err) => {
                eprintln!("xd_engine: не удалось создать окно: {err}");
                event_loop.exit();
                return;
            }
        };

        match VulkanRenderer::new(&window) {
            Ok(renderer) => self.renderer = Some(renderer),
            Err(err) => {
                eprintln!("xd_engine: не удалось создать Vulkan-рендерер: {err}");
                eprintln!("установлен ли Vulkan SDK или molten-vk (brew install molten-vk)?");
                event_loop.exit();
                return;
            }
        }

        self.window = Some(window);
        // Отсчёт времени начинается здесь, а не в `new`: между созданием
        // приложения и появлением окна проходит вся инициализация Vulkan, и
        // первый кадр получил бы её целиком в свою дельту времени
        self.last_time = Instant::now();
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(key) = event.physical_key {
                    self.track_key(key, event.state.is_pressed());

                    if key == KeyCode::Escape {
                        event_loop.exit();
                    }
                }
            }

            WindowEvent::Focused(focused) => self.focus_changed(focused),

            // Размер В ФИЗИЧЕСКИХ пикселях: поверхность вывода всегда
            // физическая, и на Retina это вдвое больше по каждой оси.
            // Свёрнутое окно приходит сюда нулевым размером — рендерер
            // переживает это сам, пропуская кадры (см. `ensure_swapchain`).
            //
            // Смена DPI отдельно не ловится намеренно: winit после
            // `ScaleFactorChanged` всё равно присылает `Resized` с уже
            // пересчитанным размером — то же решение, что и в `app.rs`
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size.width, size.height);
                }
            }

            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = now.duration_since(self.last_time).as_secs_f32();
                self.last_time = now;

                if let Some(ref mut update) = self.update_callback {
                    update(&mut self.scene, &self.pressed_keys, dt);
                }

                let Some(renderer) = self.renderer.as_mut() else {
                    return;
                };

                // Ни очистки кадра, ни depth-буфера здесь нет, в отличие от
                // CPU-пути: и то и другое делает render pass своими
                // `loadOp = CLEAR` (см. `pipeline::create_render_pass`)
                if let Err(err) = renderer.draw(&self.scene, &self.assets) {
                    eprintln!("xd_engine: ошибка рендеринга: {err}");
                    event_loop.exit();
                    return;
                }

                if self.fps_counter.tick()
                    && let Some(window) = &self.window
                {
                    window.set_title(&format!("{} | FPS: {}", WINDOW_TITLE, self.fps_counter.fps()));
                }
            }

            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Окна тут не создаётся ни одного, и это главное условие: тест обязан
    // работать без экрана и без видеокарты. Проверяется ровно то, что от
    // winit и Vulkan не зависит, — учёт зажатых клавиш. Всё остальное в этом
    // файле проверяется только запуском, ровно как и в `app.rs`

    #[test]
    fn a_held_key_is_remembered_until_released() {
        let mut app = VulkanApp::new();

        app.track_key(KeyCode::KeyW, true);
        assert!(app.pressed_keys.contains(&KeyCode::KeyW));

        // Повторное нажатие от автоповтора ОС ничего не меняет
        app.track_key(KeyCode::KeyW, true);
        assert_eq!(app.pressed_keys.len(), 1);

        app.track_key(KeyCode::KeyW, false);
        assert!(app.pressed_keys.is_empty());
    }

    #[test]
    fn losing_focus_releases_every_held_key() {
        // Тот же тест, что и у `EngineApp`, и по той же причине: без сброса
        // Alt-Tab с зажатой клавишей оставит камеру ехать навсегда.
        // Проверяются ОБЕ стороны — перепутанное условие («очищать при
        // получении фокуса») выглядит правдоподобно и оставляет баг на месте
        let mut app = VulkanApp::new();

        app.track_key(KeyCode::KeyW, true);
        app.track_key(KeyCode::ShiftLeft, true);

        app.focus_changed(true);
        assert_eq!(app.pressed_keys.len(), 2, "получение фокуса не должно ничего сбрасывать");

        app.focus_changed(false);
        assert!(app.pressed_keys.is_empty(), "потеря фокуса обязана отпустить всё");
    }
}
