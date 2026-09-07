//! `VulkanRenderer` — единственная публичная точка входа модуля: создать от
//! окна и звать `draw(&Scene, &Assets)` каждый кадр, теми же сценой и
//! аренами, что и у CPU-пути. Внутри — оркестровка всего
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

use crate::config::{DEFAULT_FAR, DEFAULT_FOV, DEFAULT_NEAR};
use crate::math::Mat4;
use crate::scene::{Assets, Instance as SceneInstance, Scene};
use crate::vulkan::depth::DepthBuffer;
use crate::vulkan::descriptor;
use crate::vulkan::device::Device;
use crate::vulkan::ffi::*;
use crate::vulkan::gpu_assets::GpuAssets;
use crate::vulkan::instance::Instance;
use crate::vulkan::loader::Library;
use crate::vulkan::pipeline::{self, Pipeline, PushConstants};
use crate::vulkan::shader;
use crate::vulkan::surface;
use crate::vulkan::swapchain::Swapchain;
use crate::vulkan::sync::FrameSync;

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
    // Зеркало `Assets` на видеокарте — буферы мешей и картинки текстур,
    // разложенные по тем же индексам. Догружается само, по мере того как
    // растут арены (см. `GpuAssets::sync`)
    gpu_assets: GpuAssets,
    // Нужны в каждом кадре, а не только при создании: любая новая аллокация
    // на GPU (меш или текстура, появившиеся в аренах на ходу) спрашивает у
    // них подходящий тип памяти. Запрашиваются один раз — у физического
    // устройства они не меняются
    memory_properties: VkPhysicalDeviceMemoryProperties,
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

        // Зеркало арен — пустое: ни одного меша и ни одной текстуры до
        // первого `draw`, потому что сцены рендерер ещё не видел. Внутри
        // сразу заводится только белая заглушка 1x1 для инстансов без
        // текстуры (см. `gpu_assets`). Командный пул нужен ей для той же
        // одноразовой заливки, что и любой другой картинке
        let gpu_assets = GpuAssets::new(&device, &memory_properties, command_pool, descriptor_set_layout)?;

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
            gpu_assets,
            memory_properties,
        })
    }

    /// Рисует один кадр сцены — второй путь к тому же миру, что и
    /// `Scene::draw` на CPU.
    ///
    /// Принимает и сцену, и арены, ровно как CPU-путь, и по той же причине:
    /// у мира и у ресурсов разные сроки жизни (CLAUDE.md, «Ресурсы и мир —
    /// разные типы»). Несколько сцен в один кадр здесь тоже возможны — но не
    /// так, как на CPU: там второй `draw` в те же буферы, а тут пришлось бы
    /// не завершать render pass между ними. Пока не понадобилось.
    ///
    /// Ждёт GPU перед началом (см. doc-комментарий модуля — почему один
    /// кадр в полёте, а не несколько)
    pub fn draw(&mut self, scene: &Scene, assets: &Assets) -> Result<(), String> {
        // Догрузить то, что появилось в аренах с прошлого кадра. Обычно это
        // два сравнения длин и ничего больше; настоящая работа случается
        // только когда игра и правда завела новый меш или текстуру
        self.gpu_assets.sync(&self.device, &self.memory_properties, self.command_pool, assets)?;

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
        self.record_command_buffer(image_index, scene)?;

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

    fn record_command_buffer(&self, image_index: u32, scene: &Scene) -> Result<(), String> {
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

        // Viewport и scissor — одни на кадр, их и правда можно задать до
        // цикла. А вот буферы с дескриптором теперь у каждого инстанса свои:
        // мешей в сцене много, текстур тоже, и «привязать один раз» больше не
        // выйдет. Это обычная цена настоящей сцены, а не потеря оптимизации —
        // сортировка инстансов по мешу и текстуре, чтобы сократить смены
        // привязок, делается позже и по замеру, а не наугад
        unsafe {
            (d.fns.cmd_set_viewport)(self.command_buffer, 0, 1, &viewport);
            (d.fns.cmd_set_scissor)(self.command_buffer, 0, 1, &scissor);
        }

        // view/projection — ОДНИ на кадр (камера у сцены одна), считаются
        // один раз вне цикла; у каждого инстанса меняется только model — и
        // то, что из него следует (MVP, матрица нормалей)
        let view_projection = &self.projection() * &scene.view_matrix();

        let offset: VkDeviceSize = 0;
        for instance in &scene.instances {
            // Проволока на GPU-пути не поддержана вовсе: линиям нужен второй
            // пайплайн с `VK_POLYGON_MODE_LINE`, а это ещё и фича устройства
            // (`fillModeNonSolid`), которую полагается спрашивать. Пропускаем,
            // но не молча — иначе инстанс просто исчезнет, и искать причину
            // будут в геометрии
            if instance.wireframe {
                WIREFRAME_UNSUPPORTED.call_once(|| {
                    eprintln!(
                        "xd_engine: Vulkan-путь не рисует проволочные инстансы — они пропущены (на CPU-пути они есть)"
                    );
                });
                continue;
            }
            if instance.face_colors.is_some() {
                FACE_COLORS_UNSUPPORTED.call_once(|| {
                    eprintln!(
                        "xd_engine: Vulkan-путь не знает про раскраску по граням — взят цвет инстанса целиком"
                    );
                });
            }

            // Пустой меш — законное состояние, а не ошибка: рисовать нечего,
            // буферов у него нет (см. `gpu_assets`)
            let Some(mesh) = self.gpu_assets.mesh(instance.mesh) else {
                continue;
            };

            let push_constants = instance_push_constants(&view_projection, instance);
            let set = self.gpu_assets.descriptor_set(instance.texture);

            unsafe {
                (d.fns.cmd_bind_vertex_buffers)(self.command_buffer, 0, 1, &mesh.vertex.handle, &offset);
                (d.fns.cmd_bind_index_buffer)(self.command_buffer, mesh.index.handle, 0, VK_INDEX_TYPE_UINT32);
                (d.fns.cmd_bind_descriptor_sets)(
                    self.command_buffer,
                    VK_PIPELINE_BIND_POINT_GRAPHICS,
                    self.pipeline.layout,
                    0,
                    1,
                    &set,
                    0,
                    std::ptr::null(),
                );
                (d.fns.cmd_push_constants)(
                    self.command_buffer,
                    self.pipeline.layout,
                    VK_SHADER_STAGE_VERTEX_BIT,
                    0,
                    std::mem::size_of::<PushConstants>() as u32,
                    &push_constants as *const PushConstants as *const std::ffi::c_void,
                );
                (d.fns.cmd_draw_indexed)(self.command_buffer, mesh.index_count, 1, 0, 0, 0);
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

    /// Матрица проекции. Камеры здесь нет вовсе — она в сцене
    /// (`Scene::view_matrix`), общая с CPU-путём.
    ///
    /// А вот проекция общей быть не может, и это не недоделка: угол обзора и
    /// плоскости отсечения берутся те же самые, из `config`, но конвенции у
    /// двух API разные — глубина `[0, 1]` вместо `[-1, 1]` и перевёрнутый Y
    /// (см. `vulkan_perspective`). Общее — то, что описывает мир; своё — то,
    /// что описывает API
    fn projection(&self) -> Mat4 {
        let aspect = self.swapchain.extent.width as f32 / self.swapchain.extent.height as f32;

        vulkan_perspective(DEFAULT_FOV, aspect, DEFAULT_NEAR, DEFAULT_FAR)
    }
}

/// Предупреждать один раз за процесс, а не каждый кадр: сообщение про
/// неподдержанную возможность полезно ровно однажды, а шестьдесят раз в
/// секунду оно превращается в помеху, за которой не видно настоящих ошибок
static WIREFRAME_UNSUPPORTED: std::sync::Once = std::sync::Once::new();
static FACE_COLORS_UNSUPPORTED: std::sync::Once = std::sync::Once::new();

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
            // Вся арена разом: буферы мешей, картинки и сэмплеры текстур,
            // пул дескрипторов. `set_layout` при этом не её — он создан
            // здесь и здесь же уничтожается ниже, потому что на него
            // ссылается ещё и layout пайплайна
            self.gpu_assets.destroy(d);
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

/// MVP, матрица нормалей и цвет одного инстанса — всё, что шейдер получает
/// про него и что меняется от инстанса к инстансу.
///
/// Модельная матрица берётся у самого инстанса (`Instance::get_model_matrix`),
/// а не собирается здесь заново: порядок компоновки T·R·S — это правило
/// движка, и второе его изложение в GPU-пути рано или поздно разошлось бы с
/// первым.
///
/// Матрица нормалей всегда считается через `normal_matrix()`, а не
/// переиспользует `model`: при равномерном масштабе они совпали бы побайтово,
/// но при НЕравномерном — нет, и грань потемнела бы там, где обязана
/// светлеть (CLAUDE.md, «Нормаль — не просто направление»). Берётся только
/// её верхний левый 3x3, а `w` каждого столбца обнуляется: транспонирование
/// обратной матрицы заносит туда перенос, который направлению ни к чему.
fn instance_push_constants(view_projection: &Mat4, instance: &SceneInstance) -> PushConstants {
    let model = instance.get_model_matrix();
    let normal_matrix = model.normal_matrix();
    let mvp = view_projection * &model;
    let nm = normal_matrix.cols;

    // Цвет инстанса — байты 0..255 на CPU-пути, а шейдер множит на яркость в
    // 0..1. Делим, а не приводим: 255 обязано дать ровно 1.0, иначе белый
    // инстанс под полным светом вышел бы чуть темнее себя
    let color = |c: u8| c as f32 / 255.0;

    PushConstants {
        mvp: mvp.cols,
        normal_matrix: [
            [nm[0][0], nm[0][1], nm[0][2], 0.0],
            [nm[1][0], nm[1][1], nm[1][2], 0.0],
            [nm[2][0], nm[2][1], nm[2][2], 0.0],
        ],
        color: [
            color(instance.color[0]),
            color(instance.color[1]),
            color(instance.color[2]),
            1.0,
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
