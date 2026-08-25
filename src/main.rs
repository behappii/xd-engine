use winit::event_loop::EventLoop;

use crate::app::EngineApp;

mod app;
mod math;
mod renderer;
mod scene;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    let mut app = EngineApp::default();

    // Запускаем приложение по новым правилам winit
    event_loop.run_app(&mut app)?;

    Ok(())
}
