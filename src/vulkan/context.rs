//! `VulkanRenderer` — единственная публичная точка входа Фазы 1: создать
//! от окна и звать `draw_frame` каждый кадр. Внутри — оркестровка всего
//! остального модуля, и здесь же, в одном месте, порядок уничтожения
//! объектов: у Vulkan он важен (framebuffer держит image view, тот —
//! swapchain, и так далее), и явный порядок в одной функции читается
//! понятнее, чем неявный порядок полей структуры — тот же выбор, что уже
//! объяснён у `Swapchain::destroy` и `FrameSync::destroy`.
//!
//! **Известный пробел Фазы 1**: изменение размера окна не пересобирает
//! swapchain — `vkAcquireNextImageKHR`/`vkQueuePresentKHR` вернут
//! `VK_ERROR_OUT_OF_DATE_KHR`, и `draw_frame` отдаст это как ошибку.
//! Пересборка swapchain на resize — материал для одной из следующих фаз,
//! не для «hello triangle»

use crate::math::{Mat4, Vec3};
use crate::scene::Mesh;
use crate::texture::Texture;
use crate::vulkan::buffer::{self, Buffer};
use crate::vulkan::depth::DepthBuffer;
use crate::vulkan::descriptor::{self, Descriptor};
use crate::vulkan::device::Device;
use crate::vulkan::ffi::*;
use crate::vulkan::image::GpuImage;
use crate::vulkan::instance::Instance;
use crate::vulkan::loader::Library;
use crate::vulkan::pipeline::{self, GpuVertex, Pipeline, PushConstants};
use crate::vulkan::sampler;
use crate::vulkan::shader;
use crate::vulkan::surface;
use crate::vulkan::swapchain::Swapchain;
use crate::vulkan::sync::FrameSync;
use std::time::Instant;

/// Статичные данные одного инстанса куба: где стоит, во сколько раз
/// растянут, с какой скоростью крутится. Не меняется кадр от кадра — только
/// вход в расчёт `PushConstants`, который зависит ещё и от текущего времени
/// (см. `VulkanRenderer::instance_push_constants`)
struct InstanceLayout {
    offset: Vec3,
    scale: Vec3,
    spin_degrees_per_second: f32,
}

// ВНИМАНИЕ: порядок полей здесь — не оформление, а корректность.
//
// Rust роняет поля структуры В ПОРЯДКЕ ОБЪЯВЛЕНИЯ, уже ПОСЛЕ того, как
// отработал ручной `Drop for VulkanRenderer` ниже. Значит три поля с
// собственными деструкторами — `device`, `instance`, `lib` — обязаны стоять
// В КОНЦЕ и именно в этом порядке: `vkDestroyDevice` раньше
// `vkDestroyInstance`, а выгрузка самой библиотеки (`dlclose` в
// `Library::drop`) — позже обоих, потому что оба зовут функции, КОТОРЫЕ В
// НЕЙ И ЛЕЖАТ.
//
// Раньше они стояли первыми — при том, что комментарий уверял в обратном, —
// и это давало настоящее падение при штатном закрытии окна:
//
//     Exception Type: EXC_BAD_ACCESS (SIGSEGV)
//     0  ???                0x119050f04 ???      ← адрес вне всех регионов
//     1  vulkan_triangle    Instance::drop + 36
//
// то есть `dlclose` уже выгрузил библиотеку, а `vkDestroyInstance` всё ещё
// звался по указателю в неё. Долго не замечалось по двум причинам: путь
// уничтожения виден только при ШТАТНОМ выходе (по Escape или крестику), а
// прибитый сигналом процесс до `Drop` не доходит вовсе; и напрямую с
// MoltenVK `dlclose` библиотеку фактически не выгружал, так что указатель
// случайно оставался рабочим. Сломалось это только с настоящим Vulkan
// Loader'ом, который выгружается по-честному
pub struct VulkanRenderer {
    surface: VkSurfaceKHR,
    swapchain: Swapchain,
    // Один буфер на всё время жизни рендерера, не на кадр в полёте — см.
    // doc-комментарий `depth::DepthBuffer` (кадр в полёте всего один).
    // Живёт рядом со swapchain, потому что размером обязан следовать за
    // ним же (оба пересчитываются вместе при изменении размера окна —
    // впрочем, пересборки на resize в этом примере всё ещё нет, см.
    // «Известный пробел Фазы 1»)
    depth: DepthBuffer,
    render_pass: VkRenderPass,
    // Layout создаётся ДО пайплайна (тот на него ссылается в
    // `VkPipelineLayoutCreateInfo`), а настоящие картинка/сэмплер/набор —
    // ПОСЛЕ него, когда есть что в набор записать. Оба поля переживают
    // рендерер целиком, поэтому и уничтожаются явно, а не как часть чего-то
    // другого — см. `Drop`
    descriptor_set_layout: VkDescriptorSetLayout,
    pipeline: Pipeline,
    framebuffers: Vec<VkFramebuffer>,
    command_pool: VkCommandPool,
    command_buffer: VkCommandBuffer,
    sync: FrameSync,
    vertex_buffer: Buffer,
    index_buffer: Buffer,
    index_count: u32,
    texture_image: GpuImage,
    sampler: VkSampler,
    descriptor: Descriptor,
    // Несколько инстансов ОДНОГО куба (общие вершинный/индексный буфер,
    // общий дескриптор текстуры) — каждый рисуется своим вызовом
    // `vkCmdDrawIndexed` со своим push-constant'ом, см.
    // `record_command_buffer`. Данные статичны, вычисляются один раз в
    // `new`, а не заново каждый кадр
    instances: Vec<InstanceLayout>,
    // Момент создания рендерера — единственный источник времени для
    // анимации кубов. `Instant`, а не счётчик кадров: поворот должен
    // зависеть от прошедшего времени, а не от FPS (тот же принцип, что у
    // `dt` в `EngineApp` на CPU-пути)
    start_time: Instant,
    // Последние три — и строго в этом порядке. Причина в комментарии над
    // структурой; трогать их местами нельзя, это не стиль
    device: Device,
    instance: Instance,
    lib: Library,
}

impl VulkanRenderer {
    pub fn new(window: &crate::winit::window::Window) -> Result<Self, String> {
        let lib = Library::load()?;

        let required_extensions = [surface::SURFACE_EXTENSION, surface::platform_extension()];
        let instance = Instance::new(&lib, "xd_engine vulkan_triangle", &required_extensions)?;

        // `scale_factor` — не косметика: на Retina без него поверхность
        // выходит вдвое мельче окна по каждой оси (см. `attach_metal_layer`
        // в `surface.rs`)
        let surface = surface::create(&lib, &instance, window, window.scale_factor())?;
        let device = Device::new(&instance, surface)?;

        let size = window.inner_size();
        let swapchain = Swapchain::new(&instance, &device, surface, (size.width, size.height))?;

        // Нужны заранее, до самой первой аллокации памяти на GPU (теперь
        // это depth-буфер, а не вершинный буфер, как раньше) — без них
        // негде узнать, какой индекс типа памяти вообще подходит под
        // требуемые свойства
        let mut memory_properties = VkPhysicalDeviceMemoryProperties {
            memory_type_count: 0,
            memory_types: [VkMemoryType::default(); VK_MAX_MEMORY_TYPES],
            memory_heap_count: 0,
            memory_heaps: [VkMemoryHeap::default(); VK_MAX_MEMORY_HEAPS],
        };
        unsafe {
            (instance.fns.get_physical_device_memory_properties)(device.physical, &mut memory_properties);
        }

        let depth = DepthBuffer::new(&device, &memory_properties, swapchain.extent)?;

        let render_pass = pipeline::create_render_pass(&device, swapchain.format)?;

        // Layout биндинга (сэмплер + картинка) нужен уже сейчас — пайплайн
        // ссылается на него в своём layout'е, хотя сама текстура появится
        // куда позже (см. doc-комментарий у поля в структуре)
        let descriptor_set_layout = descriptor::create_set_layout(&device)?;

        let vertex_module = shader::vertex_module(&device)?;
        let fragment_module = shader::fragment_module(&device)?;
        let pipeline =
            pipeline::create_pipeline(&device, render_pass, descriptor_set_layout, vertex_module, fragment_module);
        // Модули шейдеров нужны только на время сборки пайплайна — он
        // копирует себе всё нужное, а не ссылается на модуль постоянно.
        // Уничтожаем сразу, независимо от того, удался ли пайплайн:
        // держать их дальше незачем в обоих случаях
        unsafe {
            (device.fns.destroy_shader_module)(device.handle, vertex_module, std::ptr::null());
            (device.fns.destroy_shader_module)(device.handle, fragment_module, std::ptr::null());
        }
        let pipeline = pipeline?;

        let framebuffers = create_framebuffers(&device, render_pass, &swapchain, depth.view)?;

        let command_pool = create_command_pool(&device)?;
        let command_buffer = allocate_command_buffer(&device, command_pool)?;

        // Столько же семафоров `render_finished`, сколько картинок в
        // swapchain — см. doc-комментарий `sync.rs`, почему одного общего
        // не хватает даже при одном кадре в полёте
        let sync = FrameSync::new(&device, swapchain.image_views.len())?;

        // Настоящая геометрия вместо зашитого в шейдер треугольника —
        // ровно то, ради чего затевалась Фаза 2. Куб — тот же
        // `scene::Mesh::create_cube()`, что и на CPU-пути; несёт уже и
        // нормаль (Фаза 3 — освещение), и UV (Фаза 4 — текстура), причём
        // UV у `create_cube` не нулевые — разводка по граням задана ещё в
        // основном движке (см. «Развёртка куба задана четвёрками углов» в
        // CLAUDE.md)
        let cube = Mesh::create_cube();
        let vertices: Vec<GpuVertex> = cube
            .vertices
            .iter()
            .map(|v| GpuVertex {
                position: [v.position.x, v.position.y, v.position.z],
                normal: [v.normal.x, v.normal.y, v.normal.z],
                uv: [v.uv.x, v.uv.y],
            })
            .collect();
        let indices: Vec<u32> =
            cube.triangles.iter().flat_map(|t| t.iter().map(|&i| i as u32)).collect();
        let index_count = indices.len() as u32;

        let vertex_buffer =
            buffer::upload_data(&device, &memory_properties, VK_BUFFER_USAGE_VERTEX_BUFFER_BIT, &vertices)?;
        let index_buffer =
            buffer::upload_data(&device, &memory_properties, VK_BUFFER_USAGE_INDEX_BUFFER_BIT, &indices)?;

        // Шахматка вместо файла с диска — не заглушка, а намеренный выбор
        // (см. «ни файла, ни художника» у `Texture::checker`): развёртку
        // куба видно на ней сразу, а движок остаётся самодостаточным —
        // никакого пути к ассетам не нужно ни этому примеру, ни тесту.
        // Фильтр по умолчанию `Nearest`/`Nearest` — тот, что и держит
        // границы клеток резкими
        let texture = Texture::checker(8, 4, [230, 230, 230, 255], [40, 40, 60, 255]);
        let pixels = texture.level0_rgba8();
        let texture_image = GpuImage::upload_rgba8(
            &device,
            &memory_properties,
            command_pool,
            texture.width(),
            texture.height(),
            &pixels,
        )?;
        let sampler = match sampler::create(&device, texture.magnify(), texture.minify()) {
            Ok(sampler) => sampler,
            Err(err) => {
                texture_image.destroy(&device);
                return Err(err);
            }
        };
        let descriptor = match Descriptor::new(&device, descriptor_set_layout, texture_image.view, sampler) {
            Ok(descriptor) => descriptor,
            Err(err) => {
                unsafe {
                    (device.fns.destroy_sampler)(device.handle, sampler, std::ptr::null());
                }
                texture_image.destroy(&device);
                return Err(err);
            }
        };

        // Пять инстансов ОДНОГО куба — общие вершинный/индексный буфер и
        // дескриптор текстуры, разные положение/масштаб/скорость вращения.
        // Расставлены с перекрытием в экранных координатах НАРОЧНО: смысл
        // именно в том, чтобы кубы заслоняли друг друга в порядке, который
        // определяет тест глубины, а не порядок вызовов `vkCmdDrawIndexed`
        // (без него побеждал бы просто последний нарисованный — тот самый
        // класс ошибок, что depth-буфер и существует чинить). Четвёртый
        // сплющен по Y (`scale.y = 0.35`) — это тот самый случай, который
        // отличает `normal_matrix()` от простого переиспользования `model`:
        // у чистого поворота (Фаза 3) они совпадали побайтово, а здесь,
        // при неравномерном масштабе, уже нет (см. «Нормаль — не просто
        // направление» в CLAUDE.md)
        let instances = vec![
            InstanceLayout { offset: Vec3::new(-1.8, 0.0, 1.5), scale: Vec3::new(1.0, 1.0, 1.0), spin_degrees_per_second: 30.0 },
            InstanceLayout { offset: Vec3::new(0.0, 0.0, 0.0), scale: Vec3::new(1.0, 1.0, 1.0), spin_degrees_per_second: 45.0 },
            InstanceLayout { offset: Vec3::new(1.3, 0.4, -1.8), scale: Vec3::new(1.0, 1.0, 1.0), spin_degrees_per_second: 60.0 },
            InstanceLayout { offset: Vec3::new(-0.7, -1.0, -1.2), scale: Vec3::new(1.0, 0.35, 1.0), spin_degrees_per_second: 20.0 },
            InstanceLayout { offset: Vec3::new(2.3, 0.7, 2.0), scale: Vec3::new(0.6, 0.6, 0.6), spin_degrees_per_second: -35.0 },
        ];

        Ok(Self {
            lib,
            instance,
            surface,
            device,
            swapchain,
            depth,
            render_pass,
            descriptor_set_layout,
            pipeline,
            framebuffers,
            command_pool,
            command_buffer,
            sync,
            vertex_buffer,
            index_buffer,
            index_count,
            texture_image,
            sampler,
            descriptor,
            instances,
            start_time: Instant::now(),
        })
    }

    /// Рисует один кадр: несколько инстансов куба поверх тёмно-синего фона.
    /// Ждёт GPU перед началом (см. doc-комментарий модуля — почему один
    /// кадр в полёте, а не несколько) — цена этого ожидания на пяти кубах
    /// по-прежнему не видна, оптимизировать пока нечего
    pub fn draw_frame(&mut self) -> Result<(), String> {
        let d = &self.device;

        unsafe {
            (d.fns.wait_for_fences)(d.handle, 1, &self.sync.in_flight, VK_TRUE, u64::MAX);
            (d.fns.reset_fences)(d.handle, 1, &self.sync.in_flight);
        }

        let mut image_index = 0u32;
        let acquire_result = unsafe {
            (d.fns.acquire_next_image_khr)(
                d.handle,
                self.swapchain.handle,
                u64::MAX,
                self.sync.image_available,
                VkFence::NULL,
                &mut image_index,
            )
        };
        if acquire_result != VK_SUCCESS && acquire_result != VK_SUBOPTIMAL_KHR {
            return Err(format!(
                "vkAcquireNextImageKHR вернул {acquire_result} — пересборка swapchain на resize не реализована в Фазе 1"
            ));
        }

        unsafe {
            (d.fns.reset_command_buffer)(self.command_buffer, 0);
        }
        self.record_command_buffer(image_index)?;

        // Семафор берётся по номеру ПОЛУЧЕННОЙ картинки, а не один общий:
        // показ предыдущего кадра мог ещё держать свой (см. doc-комментарий
        // `sync.rs`). Копия в локальную переменную нужна затем, что и
        // `vkQueueSubmit`, и `vkQueuePresentKHR` хотят УКАЗАТЕЛЬ на семафор,
        // а не сам семафор
        let render_finished = self.sync.render_finished[image_index as usize];

        let wait_stage = VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT;
        let submit_info = VkSubmitInfo {
            s_type: VK_STRUCTURE_TYPE_SUBMIT_INFO,
            p_next: std::ptr::null(),
            wait_semaphore_count: 1,
            p_wait_semaphores: &self.sync.image_available,
            p_wait_dst_stage_mask: &wait_stage,
            command_buffer_count: 1,
            p_command_buffers: &self.command_buffer,
            signal_semaphore_count: 1,
            p_signal_semaphores: &render_finished,
        };
        let result = unsafe { (d.fns.queue_submit)(d.queue, 1, &submit_info, self.sync.in_flight) };
        if result != VK_SUCCESS {
            return Err(format!("vkQueueSubmit вернул {result}"));
        }

        let present_info = VkPresentInfoKHR {
            s_type: VK_STRUCTURE_TYPE_PRESENT_INFO_KHR,
            p_next: std::ptr::null(),
            wait_semaphore_count: 1,
            p_wait_semaphores: &render_finished,
            swapchain_count: 1,
            p_swapchains: &self.swapchain.handle,
            p_image_indices: &image_index,
            p_results: std::ptr::null_mut(),
        };
        let result = unsafe { (d.fns.queue_present_khr)(d.queue, &present_info) };
        if result != VK_SUCCESS && result != VK_SUBOPTIMAL_KHR {
            return Err(format!("vkQueuePresentKHR вернул {result}"));
        }

        Ok(())
    }

    fn record_command_buffer(&self, image_index: u32) -> Result<(), String> {
        let d = &self.device;

        let begin_info =
            VkCommandBufferBeginInfo { s_type: VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO, p_next: std::ptr::null(), flags: 0, p_inheritance_info: std::ptr::null() };
        let result = unsafe { (d.fns.begin_command_buffer)(self.command_buffer, &begin_info) };
        if result != VK_SUCCESS {
            return Err(format!("vkBeginCommandBuffer вернул {result}"));
        }

        // Тёмно-синий фон — кубы (оранжевая шахматка, см. `shader.rs`)
        // обязаны быть видны на нём безошибочно. Глубина чистится в
        // 1.0 — «максимально далеко» при конвенции Vulkan `[0,1]`
        // (см. `VK_COMPARE_OP_LESS` в `pipeline.rs`): без этого первый же
        // тест сравнил бы честную глубину куба с мусором, а не с «здесь
        // ещё ничего не рисовали». Порядок значений в массиве ОБЯЗАН
        // совпадать с порядком attachment'ов в `create_render_pass`
        // (индекс 0 — цвет, 1 — глубина), а не с тем, что естественнее
        // для чтения
        let clear_values = [
            VkClearValue { color: VkClearColorValue { float32: [0.01, 0.01, 0.03, 1.0] } },
            VkClearValue { depth_stencil: VkClearDepthStencilValue { depth: 1.0, stencil: 0 } },
        ];
        let render_area = VkRect2D { offset: VkOffset2D { x: 0, y: 0 }, extent: self.swapchain.extent };
        let render_pass_begin = VkRenderPassBeginInfo {
            s_type: VK_STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO,
            p_next: std::ptr::null(),
            render_pass: self.render_pass,
            framebuffer: self.framebuffers[image_index as usize],
            render_area,
            clear_value_count: clear_values.len() as u32,
            p_clear_values: clear_values.as_ptr(),
        };
        unsafe {
            (d.fns.cmd_begin_render_pass)(self.command_buffer, &render_pass_begin, VK_SUBPASS_CONTENTS_INLINE);
            (d.fns.cmd_bind_pipeline)(self.command_buffer, VK_PIPELINE_BIND_POINT_GRAPHICS, self.pipeline.handle);
        }

        let viewport = VkViewport {
            x: 0.0,
            y: 0.0,
            width: self.swapchain.extent.width as f32,
            height: self.swapchain.extent.height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        let scissor = VkRect2D { offset: VkOffset2D { x: 0, y: 0 }, extent: self.swapchain.extent };

        // Вершинный/индексный буфер и дескриптор текстуры — ОБЩИЕ на все
        // инстансы (одна и та же геометрия, одна и та же шахматка), поэтому
        // биндятся один раз ДО цикла, а не в нём: перебиндивать одно и то же
        // на каждый инстанс было бы лишними вызовами без единого эффекта
        let offset: VkDeviceSize = 0;
        unsafe {
            (d.fns.cmd_set_viewport)(self.command_buffer, 0, 1, &viewport);
            (d.fns.cmd_set_scissor)(self.command_buffer, 0, 1, &scissor);
            (d.fns.cmd_bind_vertex_buffers)(self.command_buffer, 0, 1, &self.vertex_buffer.handle, &offset);
            (d.fns.cmd_bind_index_buffer)(self.command_buffer, self.index_buffer.handle, 0, VK_INDEX_TYPE_UINT32);
            (d.fns.cmd_bind_descriptor_sets)(
                self.command_buffer,
                VK_PIPELINE_BIND_POINT_GRAPHICS,
                self.pipeline.layout,
                0,
                1,
                &self.descriptor.set,
                0,
                std::ptr::null(),
            );
        }

        // view/projection — ОДНИ на кадр (камера у всех инстансов общая),
        // считаются один раз вне цикла; у каждого инстанса меняется только
        // model — и то, что из него следует (MVP, матрица нормалей)
        let elapsed = self.start_time.elapsed().as_secs_f32();
        let (view, projection) = self.view_projection();
        let view_projection = &projection * &view;

        for layout in &self.instances {
            let push_constants = instance_push_constants(&view_projection, layout, elapsed);
            unsafe {
                (d.fns.cmd_push_constants)(
                    self.command_buffer,
                    self.pipeline.layout,
                    VK_SHADER_STAGE_VERTEX_BIT,
                    0,
                    std::mem::size_of::<PushConstants>() as u32,
                    &push_constants as *const PushConstants as *const std::ffi::c_void,
                );
                (d.fns.cmd_draw_indexed)(self.command_buffer, self.index_count, 1, 0, 0, 0);
            }
        }

        unsafe {
            (d.fns.cmd_end_render_pass)(self.command_buffer);
        }

        let result = unsafe { (d.fns.end_command_buffer)(self.command_buffer) };
        if result != VK_SUCCESS {
            return Err(format!("vkEndCommandBuffer вернул {result}"));
        }
        Ok(())
    }

    /// Камера — общая для всех инстансов кадра, поэтому вынесена из
    /// расчёта push-constant'ов одного инстанса (`instance_push_constants`)
    /// в отдельный метод: считать один и тот же `view`/`projection` пять
    /// раз за кадр (по числу кубов) было бы не ошибкой, а просто лишней
    /// работой без единого отличия в результате
    fn view_projection(&self) -> (Mat4, Mat4) {
        // Отодвинута дальше и чуть приподнята относительно Фазы 2-4
        // (там была одна-единственная камера у начала координат) — пятерым
        // кубам, разложенным с перекрытием (см. `instances` в `new`),
        // нужен обзор шире, чем одному кубу в центре кадра
        let eye = Vec3::new(0.0, 1.5, 9.0);
        let view = Mat4::look_at(eye, Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));

        let aspect = self.swapchain.extent.width as f32 / self.swapchain.extent.height as f32;
        let projection = vulkan_perspective(60.0, aspect, 0.1, 100.0);

        (view, projection)
    }
}

impl Drop for VulkanRenderer {
    fn drop(&mut self) {
        let d = &self.device;
        unsafe {
            // Дожидаемся GPU ДО того, как начнём разрушать хоть что-то —
            // `Device::drop` тоже об этом позаботится, но он сработает уже
            // ПОСЛЕ этого блока (поля роняются после тела `drop`), а
            // framebuffer/pipeline/render pass уничтожаются здесь, раньше
            (d.fns.device_wait_idle)(d.handle);

            self.sync.destroy(d);
            self.vertex_buffer.destroy(d);
            self.index_buffer.destroy(d);
            self.descriptor.destroy(d);
            (d.fns.destroy_sampler)(d.handle, self.sampler, std::ptr::null());
            self.texture_image.destroy(d);
            (d.fns.destroy_command_pool)(d.handle, self.command_pool, std::ptr::null());
            for &framebuffer in &self.framebuffers {
                (d.fns.destroy_framebuffer)(d.handle, framebuffer, std::ptr::null());
            }
            (d.fns.destroy_pipeline)(d.handle, self.pipeline.handle, std::ptr::null());
            (d.fns.destroy_pipeline_layout)(d.handle, self.pipeline.layout, std::ptr::null());
            (d.fns.destroy_descriptor_set_layout)(d.handle, self.descriptor_set_layout, std::ptr::null());
            (d.fns.destroy_render_pass)(d.handle, self.render_pass, std::ptr::null());
            self.depth.destroy(d);
            self.swapchain.destroy(d);
            (self.instance.fns.destroy_surface_khr)(self.instance.handle, self.surface, std::ptr::null());
        }
        // `device`, `instance` и `lib` уничтожатся следом сами — у них есть
        // собственный `Drop` (см. `device.rs`/`instance.rs`/`loader.rs`), и
        // порядок полей структуры ставит их последними, именно в таком
        // порядке. Это и есть то самое место, ради которого написан
        // комментарий над объявлением структуры: раньше здесь стояло то же
        // утверждение, а поля лежали наоборот, и штатное закрытие окна
        // валилось с SIGSEGV
    }
}

/// MVP и матрица нормалей одного инстанса на текущий момент — каждый куб
/// крутится вокруг Y со своей скоростью (`InstanceLayout::spin_degrees_per_second`),
/// не для красоты, а как самая простая проверка, что push-constant и правда
/// доезжает до шейдера заново на каждый вызов `vkCmdDrawIndexed`, а не
/// читается один раз и застывает на всех инстансах разом.
///
/// `model = T(offset) · Rx(20°) · Ry(angle) · S(scale)` — тот же порядок
/// компоновки T·R·S, что у `Instance::get_model_matrix` на CPU-пути (см.
/// CLAUDE.md), только без Rz (эта демонстрация крутит только вокруг Y).
///
/// Матрица нормалей всегда считается через `normal_matrix()`, а не
/// переиспользует `model` напрямую: у ЧЕТЫРЁХ инстансов масштаб
/// единичный или равномерный, и там `normal_matrix()` совпал бы с `model`
/// побайтово (см. «Нормаль — не просто направление» в CLAUDE.md — как раз
/// то, что уже наблюдалось в Фазе 3 на одиночном кубе), но у ПЯТОГО
/// (`scale.y = 0.35`, сплющен) — нет: наивное переиспользование `model`
/// там дало бы неверно повёрнутые нормали и грань, темнеющую там, где
/// обязана светлеть, — ровно баг, которого больше нет благодаря тесту
/// `squashing_an_object_turns_its_normals_towards_the_light` на CPU-пути.
fn instance_push_constants(view_projection: &Mat4, layout: &InstanceLayout, elapsed: f32) -> PushConstants {
    let spin_angle = elapsed * layout.spin_degrees_per_second;

    let rotation = &Mat4::rotation_x(20.0) * &Mat4::rotation_y(spin_angle);
    let scale_mat = Mat4::scaling(layout.scale.x, layout.scale.y, layout.scale.z);
    let translation = Mat4::translation(layout.offset.x, layout.offset.y, layout.offset.z);
    let model = &translation * &(&rotation * &scale_mat);

    let normal_matrix = model.normal_matrix();
    let mvp = view_projection * &model;
    let nm = normal_matrix.cols;

    PushConstants {
        mvp: mvp.cols,
        normal_matrix: [
            [nm[0][0], nm[0][1], nm[0][2], 0.0],
            [nm[1][0], nm[1][1], nm[1][2], 0.0],
            [nm[2][0], nm[2][1], nm[2][2], 0.0],
        ],
    }
}

/// Проекция для Vulkan — не `math::Mat4::perspective`.
///
/// Та матрица посчитана под конвенцию глубины OpenGL: после перспективного
/// деления её z попадает в `[-1, 1]` (см. тест
/// `perspective_maps_near_and_far_to_ndc_range` в `math/mat4.rs`). Годится
/// для клиппинга этого движка на CPU, но Vulkan аппаратно ждёт z в `[0, 1]`
/// — всё, что попало бы в `[-1, 0)`, будет молча отсечено как «перед ближней
/// плоскостью», и часть куба обязана была бы пропасть.
///
/// Формула выведена из тех же двух граничных условий, что и оригинал
/// (`-near` обязан дать 0, `-far` обязан дать 1), просто под другой целевой
/// диапазон — числитель короче ровно потому, что не нужно центрировать
/// его вокруг нуля.
///
/// `scale_y` вдобавок ОТРИЦАТЕЛЬНЫЙ — второе отличие от OpenGL: ось Y
/// клип-пространства у Vulkan перевёрнута относительно экрана (NDC `y=-1`
/// — низ вьюпорта, а не верх). Без минуса картинка вышла бы верной по
/// форме, но вверх ногами
fn vulkan_perspective(fov_degrees: f32, aspect_ratio: f32, near: f32, far: f32) -> Mat4 {
    let fov_radians = fov_degrees.to_radians();
    let tan_half_fov = (fov_radians / 2.0).tan();

    let scale_x = 1.0 / (tan_half_fov * aspect_ratio);
    let scale_y = -1.0 / tan_half_fov;

    let remap_z = far / (near - far);
    let remap_w = (far * near) / (near - far);

    Mat4 {
        cols: [
            [scale_x, 0.0, 0.0, 0.0],
            [0.0, scale_y, 0.0, 0.0],
            [0.0, 0.0, remap_z, -1.0],
            [0.0, 0.0, remap_w, 0.0],
        ],
    }
}

/// Один `depth_view` на ВСЕ framebuffer'ы — то же самое единственное
/// изображение глубины подставляется в каждый из них. Framebuffer'ов
/// столько же, сколько картинок в swapchain, но глубина на экран не идёт
/// и не обязана быть отдельной под каждую (см. doc-комментарий
/// `depth::DepthBuffer`)
fn create_framebuffers(
    device: &Device,
    render_pass: VkRenderPass,
    swapchain: &Swapchain,
    depth_view: VkImageView,
) -> Result<Vec<VkFramebuffer>, String> {
    let mut framebuffers = Vec::with_capacity(swapchain.image_views.len());
    for &color_view in &swapchain.image_views {
        // Порядок ОБЯЗАН совпадать с `p_attachments` в `create_render_pass`
        // (индекс 0 — цвет, 1 — глубина) — framebuffer сам по себе никак
        // не проверяет, что вложение на своём месте, просто пишет по
        // индексу
        let attachments = [color_view, depth_view];
        let create_info = VkFramebufferCreateInfo {
            s_type: VK_STRUCTURE_TYPE_FRAMEBUFFER_CREATE_INFO,
            p_next: std::ptr::null(),
            flags: 0,
            render_pass,
            attachment_count: attachments.len() as u32,
            p_attachments: attachments.as_ptr(),
            width: swapchain.extent.width,
            height: swapchain.extent.height,
            layers: 1,
        };
        let mut framebuffer = VkFramebuffer::NULL;
        let result =
            unsafe { (device.fns.create_framebuffer)(device.handle, &create_info, std::ptr::null(), &mut framebuffer) };
        if result != VK_SUCCESS {
            return Err(format!("vkCreateFramebuffer вернул {result}"));
        }
        framebuffers.push(framebuffer);
    }
    Ok(framebuffers)
}

fn create_command_pool(device: &Device) -> Result<VkCommandPool, String> {
    let create_info = VkCommandPoolCreateInfo {
        s_type: VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,
        p_next: std::ptr::null(),
        // RESET_COMMAND_BUFFER: разрешает `vkResetCommandBuffer` на
        // отдельном буфере — без флага пришлось бы каждый раз пересоздавать
        // пул целиком, а не просто сбрасывать один буфер перед новой записью
        flags: VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT,
        queue_family_index: device.queue_family,
    };
    let mut pool = VkCommandPool::NULL;
    let result = unsafe { (device.fns.create_command_pool)(device.handle, &create_info, std::ptr::null(), &mut pool) };
    if result != VK_SUCCESS {
        return Err(format!("vkCreateCommandPool вернул {result}"));
    }
    Ok(pool)
}

fn allocate_command_buffer(device: &Device, pool: VkCommandPool) -> Result<VkCommandBuffer, String> {
    let allocate_info = VkCommandBufferAllocateInfo {
        s_type: VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
        p_next: std::ptr::null(),
        command_pool: pool,
        level: VK_COMMAND_BUFFER_LEVEL_PRIMARY,
        command_buffer_count: 1,
    };
    let mut buffer = VkCommandBuffer::NULL;
    let result = unsafe { (device.fns.allocate_command_buffers)(device.handle, &allocate_info, &mut buffer) };
    if result != VK_SUCCESS {
        return Err(format!("vkAllocateCommandBuffers вернул {result}"));
    }
    Ok(buffer)
}
