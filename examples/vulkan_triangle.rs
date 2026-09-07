//! Фаза 1 GPU-бэкенда — «hello triangle» на голом Vulkan.
//!
//! Нарочно не через `EngineApp`: этот пример не трогает CPU-путь ни
//! строкой и не заводит `pixels` вовсе — вывод кадра идёт напрямую в
//! swapchain через `VulkanRenderer` (`xd_engine::vulkan`). Своё окно и свой
//! `ApplicationHandler`, как у `EngineApp`, но без буфера кадра и без
//! глубины — их здесь не существует, кадр рисует GPU.
//!
//! Через `xd_engine::winit`, а не `use winit` напрямую — та же причина,
//! что у всех остальных примеров: так пример продолжает моделировать
//! чужой проект, у которого прямой зависимости на `winit` нет (см.
//! соответствующий инвариант в CLAUDE.md).
//!
//! Требует установленного Vulkan (Vulkan SDK для macOS/LunarG или
//! `brew install molten-vk` — на Mac этот путь идёт через MoltenVK, см.
//! doc-комментарий `xd_engine::vulkan`).

use xd_engine::vulkan::VulkanRenderer;
use xd_engine::winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

#[derive(Default)]
struct TriangleApp {
    window: Option<Window>,
    renderer: Option<VulkanRenderer>,
}

impl ApplicationHandler for TriangleApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window_attributes = Window::default_attributes()
            .with_title("xd_engine — Vulkan hello triangle")
            .with_inner_size(LogicalSize::new(800.0, 600.0));
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
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::KeyboardInput { event, .. } => {
                if event.physical_key == PhysicalKey::Code(KeyCode::Escape) {
                    event_loop.exit();
                }
            }

            WindowEvent::RedrawRequested => {
                if let Some(renderer) = self.renderer.as_mut() {
                    if let Err(err) = renderer.draw_frame() {
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

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let mut app = TriangleApp::default();
    event_loop.run_app(&mut app).unwrap();
}
