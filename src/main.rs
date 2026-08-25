use winit::event_loop::EventLoop;

use crate::{
    app::EngineApp,
    math::Vec3,
    scene::{Instance, Mesh},
};

mod app;
mod math;
mod renderer;
mod scene;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;

    // Создаем Движок
    let mut app = EngineApp::new();

    // Генерируем меши
    let cube = Mesh::create_cube();
    let pyramid = Mesh::create_pyramid();

    // Создаем инстансы
    let mut obj1 = Instance::new(cube, Vec3::new(-1.8, 0.0, 0.0));
    obj1.scale = Vec3::new(1.2, 1.2, 1.2);

    let mut obj2 = Instance::new(pyramid, Vec3::new(1.8, 0.0, 0.0));
    obj2.scale = Vec3::new(1.4, 1.4, 1.4);

    // Легко добавляем третий объект (еще один куб повыше), просто написав одну строчку!
    let mut obj3 = Instance::new(Mesh::create_cube(), Vec3::new(0.0, 1.5, -1.0));
    obj3.scale = Vec3::new(0.6, 0.6, 0.6);

    // Закидываем инстансы в сцену движка
    app.scene.add_instance(obj1);
    app.scene.add_instance(obj2);
    app.scene.add_instance(obj3);

    // Запускаем приложение
    event_loop.run_app(&mut app)?;

    Ok(())
}
