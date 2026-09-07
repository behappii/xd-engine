//! Render pass и графический пайплайн. В Фазе 1 здесь был один цветовой
//! attachment без глубины, вершинных буферов и дескрипторов — вершины были
//! зашиты прямо в шейдер. С тех пор добавились вершинный/индексный буфер и
//! push-constant (Фаза 2), матрица нормалей в том же push-constant'е
//! (Фаза 3), дескриптор текстуры (Фаза 4) и, наконец, второй attachment —
//! глубина, вместе с тестом в пайплайне (Фаза 5, `create_render_pass`/
//! `depth_stencil` ниже).
//!
//! `viewport`/`scissor` объявлены динамическими (`VkPipelineDynamicStateCreateInfo`),
//! а не зашиты в сам пайплайн — иначе изменение размера окна требовало бы
//! пересобрать весь пайплайн целиком, а не только framebuffer/swapchain

use crate::vulkan::device::Device;
use crate::vulkan::ffi::*;

/// Вершина, как её видит GPU — НЕ то же самое, что `scene::Vertex`.
///
/// `scene::Vertex` не помечен `#[repr(C)]` (ему это не нужно, он живёт
/// только на CPU и никогда не копируется как сырые байты), поэтому
/// полагаться на порядок его полей в памяти нельзя — компилятор волен
/// переставить их местами. Здесь раскладка обязана быть точной и стабильной
/// (`VkVertexInputAttributeDescription::offset` ссылается на неё руками),
/// поэтому у GPU-вершины свой тип. Нормаль — второй атрибут того же
/// binding (Фаза 3, освещение), UV — третий (Фаза 4, текстура)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GpuVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

/// Побайтовый вид `Mat4::cols` (`math/mat4.rs`) — четыре столбца по 4×f32,
/// ровно то, что шейдер (`shader.rs`) читает как четыре push-constant
/// vec4. Тип, а не голое число 64: размер массива посчитан компилятором,
/// а не переписан руками в двух местах (здесь и в `context.rs`) с риском
/// разъехаться
pub type Mat4Bytes = [[f32; 4]; 4];

/// Матрица нормалей несёт только верхний левый 3×3 (`Mat4::transform_dir`
/// читает лишь его — см. CLAUDE.md, «Точка против направления»), но каждый
/// столбец всё равно занимает целый vec4: push-constant/uniform блок в
/// SPIR-V выравнивает элементы по 16 байт, и класть vec3 без паддинга в
/// такой блок значило бы врать шейдеру о его собственной раскладке. Четвёртая
/// компонента каждого столбца поэтому всегда 0.0 — она не мусор, а то самое
/// значение, что и обнуляет вклад `w` при вычислении в `shader.rs`
pub type NormalMatrixBytes = [[f32; 4]; 3];

/// Всё, что `record_command_buffer` кладёт в push-constant одним куском:
/// MVP для позиции, матрица нормалей для освещения и цвет инстанса —
/// вместе, потому что это ОДИН push-constant блок в шейдере (`shader.rs`), а
/// не три раздельных. `#[repr(C)]` без паддинга между полями: все три —
/// массивы `f32` с выравниванием 4, и 128 байт получаются ровно теми же
/// восемью vec4-слотами (offset 0, 16, …, 112), что в SPIR-V — считать
/// `size_of` от одного типа, а не от суммы чисел в двух файлах.
///
/// **128 байт — это ровно потолок, и следующее поле сюда уже не влезет.**
/// Спецификация гарантирует `maxPushConstantsSize` не меньше 128 на ЛЮБОЙ
/// реализации, но и не больше: устройства с бо́льшим лимитом бывают, а
/// рассчитывать на них нельзя, не спросив. Пока данных на инстанс ровно
/// столько — push-constant остаётся самым дешёвым способом их доставить, без
/// буфера, дескриптора и выравниваний. Когда понадобится ещё что-нибудь
/// (индекс материала, вторая матрица), правильный ход не «а вдруг влезет», а
/// переезд на uniform-буфер с динамическим смещением: один буфер на кадр,
/// смещение на инстанс
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PushConstants {
    pub mvp: Mat4Bytes,
    pub normal_matrix: NormalMatrixBytes,
    /// Цвет инстанса в 0..1, четвёртая компонента не используется (альфы в
    /// пайплайне нет вовсе — CLAUDE.md, «Залитые пиксели всегда пишут альфу
    /// 255»), но слот всё равно занимает целый vec4: блок выравнивает
    /// элементы по 16 байт
    pub color: [f32; 4],
}

/// Render pass с двумя attachment'ами — цветом и глубиной (Фаза 5): цвет
/// загружается и хранится как раньше (`CLEAR`/`STORE`, иначе презентовать
/// будет нечего), глубина ТОЖЕ загружается через `CLEAR` (иначе тест
/// глубины сравнивал бы с мусором прошлого кадра), но хранить её незачем —
/// `DONT_CARE`, на экран она не идёт и следующему кадру не нужна (см.
/// doc-комментарий `depth::DepthBuffer` — буфер один на все кадры и
/// перезаписывается целиком каждый раз)
pub fn create_render_pass(device: &Device, swapchain_format: VkEnum) -> Result<VkRenderPass, String> {
    let color_attachment = VkAttachmentDescription {
        flags: 0,
        format: swapchain_format,
        samples: VK_SAMPLE_COUNT_1_BIT,
        load_op: VK_ATTACHMENT_LOAD_OP_CLEAR,
        store_op: VK_ATTACHMENT_STORE_OP_STORE,
        stencil_load_op: VK_ATTACHMENT_LOAD_OP_DONT_CARE,
        stencil_store_op: VK_ATTACHMENT_STORE_OP_DONT_CARE,
        initial_layout: VK_IMAGE_LAYOUT_UNDEFINED,
        final_layout: VK_IMAGE_LAYOUT_PRESENT_SRC_KHR,
    };
    let depth_attachment = VkAttachmentDescription {
        flags: 0,
        format: VK_FORMAT_D32_SFLOAT,
        samples: VK_SAMPLE_COUNT_1_BIT,
        load_op: VK_ATTACHMENT_LOAD_OP_CLEAR,
        store_op: VK_ATTACHMENT_STORE_OP_DONT_CARE,
        stencil_load_op: VK_ATTACHMENT_LOAD_OP_DONT_CARE,
        stencil_store_op: VK_ATTACHMENT_STORE_OP_DONT_CARE,
        initial_layout: VK_IMAGE_LAYOUT_UNDEFINED,
        final_layout: VK_IMAGE_LAYOUT_DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
    };
    let attachments = [color_attachment, depth_attachment];

    let color_ref = VkAttachmentReference { attachment: 0, layout: VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL };
    let depth_ref =
        VkAttachmentReference { attachment: 1, layout: VK_IMAGE_LAYOUT_DEPTH_STENCIL_ATTACHMENT_OPTIMAL };

    let subpass = VkSubpassDescription {
        flags: 0,
        pipeline_bind_point: VK_PIPELINE_BIND_POINT_GRAPHICS,
        input_attachment_count: 0,
        p_input_attachments: std::ptr::null(),
        color_attachment_count: 1,
        p_color_attachments: &color_ref,
        p_resolve_attachments: std::ptr::null(),
        p_depth_stencil_attachment: &depth_ref,
        preserve_attachment_count: 0,
        p_preserve_attachments: std::ptr::null(),
    };

    // Без этой зависимости запись могла бы начаться РАНЬШЕ, чем swapchain
    // реально отдаст картинку (acquire — это только сигнал семафора, сама
    // передача владения картинкой происходит асинхронно) — классический
    // источник гонки, которую тут решает не отдельный barrier, а
    // зависимость подпасса от внешнего "ничего". Стадии/маски теперь
    // покрывают ОБА attachment'а: `EARLY_FRAGMENT_TESTS` — стадия, где
    // Vulkan делает ранний тест и запись глубины (до самого фрагментного
    // шейдера), и её не заменяет `COLOR_ATTACHMENT_OUTPUT`, которая к
    // глубине отношения не имеет вовсе
    let dependency = VkSubpassDependency {
        src_subpass: VK_SUBPASS_EXTERNAL,
        dst_subpass: 0,
        src_stage_mask: VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT | VK_PIPELINE_STAGE_EARLY_FRAGMENT_TESTS_BIT,
        dst_stage_mask: VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT | VK_PIPELINE_STAGE_EARLY_FRAGMENT_TESTS_BIT,
        src_access_mask: 0,
        dst_access_mask: VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT | VK_ACCESS_DEPTH_STENCIL_ATTACHMENT_WRITE_BIT,
        dependency_flags: 0,
    };

    let create_info = VkRenderPassCreateInfo {
        s_type: VK_STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        attachment_count: attachments.len() as u32,
        p_attachments: attachments.as_ptr(),
        subpass_count: 1,
        p_subpasses: &subpass,
        dependency_count: 1,
        p_dependencies: &dependency,
    };

    let mut render_pass = VkRenderPass::NULL;
    let result =
        unsafe { (device.fns.create_render_pass)(device.handle, &create_info, std::ptr::null(), &mut render_pass) };
    if result != VK_SUCCESS {
        return Err(format!("vkCreateRenderPass вернул {result}"));
    }
    Ok(render_pass)
}

pub struct Pipeline {
    pub layout: VkPipelineLayout,
    pub handle: VkPipeline,
}

pub fn create_pipeline(
    device: &Device,
    render_pass: VkRenderPass,
    descriptor_set_layout: VkDescriptorSetLayout,
    vertex_module: VkShaderModule,
    fragment_module: VkShaderModule,
) -> Result<Pipeline, String> {
    let entry_point = c"main";

    let stages = [
        VkPipelineShaderStageCreateInfo {
            s_type: VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
            p_next: std::ptr::null(),
            flags: 0,
            stage: VK_SHADER_STAGE_VERTEX_BIT,
            module: vertex_module,
            p_name: entry_point.as_ptr(),
            p_specialization_info: std::ptr::null(),
        },
        VkPipelineShaderStageCreateInfo {
            s_type: VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
            p_next: std::ptr::null(),
            flags: 0,
            stage: VK_SHADER_STAGE_FRAGMENT_BIT,
            module: fragment_module,
            p_name: entry_point.as_ptr(),
            p_specialization_info: std::ptr::null(),
        },
    ];

    // Один binding (интерливинг позиции/нормали/UV в одном буфере, шаг —
    // размер GpuVertex целиком), три атрибута на разных смещениях внутри
    // него
    let binding = VkVertexInputBindingDescription {
        binding: 0,
        stride: std::mem::size_of::<GpuVertex>() as u32,
        input_rate: VK_VERTEX_INPUT_RATE_VERTEX,
    };
    let attributes = [
        VkVertexInputAttributeDescription {
            location: 0,
            binding: 0,
            format: VK_FORMAT_R32G32B32_SFLOAT,
            offset: std::mem::offset_of!(GpuVertex, position) as u32,
        },
        VkVertexInputAttributeDescription {
            location: 1,
            binding: 0,
            format: VK_FORMAT_R32G32B32_SFLOAT,
            offset: std::mem::offset_of!(GpuVertex, normal) as u32,
        },
        VkVertexInputAttributeDescription {
            location: 2,
            binding: 0,
            format: VK_FORMAT_R32G32_SFLOAT,
            offset: std::mem::offset_of!(GpuVertex, uv) as u32,
        },
    ];
    let vertex_input = VkPipelineVertexInputStateCreateInfo {
        s_type: VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        vertex_binding_description_count: 1,
        p_vertex_binding_descriptions: &binding,
        vertex_attribute_description_count: attributes.len() as u32,
        p_vertex_attribute_descriptions: attributes.as_ptr(),
    };

    let input_assembly = VkPipelineInputAssemblyStateCreateInfo {
        s_type: VK_STRUCTURE_TYPE_PIPELINE_INPUT_ASSEMBLY_STATE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        topology: VK_PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
        primitive_restart_enable: VK_FALSE,
    };

    // Значения полей не важны — реальные `VkViewport`/`VkRect2D` каждый
    // кадр задаёт `vkCmdSetViewport`/`vkCmdSetScissor` (см. `context.rs`),
    // здесь нужны только СЧЁТЧИКИ (по одному), а не сами данные
    let viewport_state = VkPipelineViewportStateCreateInfo {
        s_type: VK_STRUCTURE_TYPE_PIPELINE_VIEWPORT_STATE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        viewport_count: 1,
        p_viewports: std::ptr::null(),
        scissor_count: 1,
        p_scissors: std::ptr::null(),
    };

    let rasterization = VkPipelineRasterizationStateCreateInfo {
        s_type: VK_STRUCTURE_TYPE_PIPELINE_RASTERIZATION_STATE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        depth_clamp_enable: VK_FALSE,
        rasterizer_discard_enable: VK_FALSE,
        polygon_mode: VK_POLYGON_MODE_FILL,
        // BACK, а не NONE (как было в Фазе 1 для зашитого в шейдер
        // треугольника без гарантированного обхода): `scene::Mesh::create_cube`
        // строит грани строго против часовой стрелки при взгляде снаружи
        // (см. CLAUDE.md, «Порядок обхода»), и без culling'а обратные грани
        // остаются в кадре наравне с лицевыми. До Фазы 5 глубины не было —
        // отбраковка обратных граней снимала только часть путаницы; теперь
        // с настоящим тестом глубины (`depth_stencil` ниже) верный порядок
        // держится сам, culling просто экономит заливку невидимых граней
        cull_mode: VK_CULL_MODE_BACK_BIT,
        front_face: VK_FRONT_FACE_COUNTER_CLOCKWISE,
        depth_bias_enable: VK_FALSE,
        depth_bias_constant_factor: 0.0,
        depth_bias_clamp: 0.0,
        depth_bias_slope_factor: 0.0,
        line_width: 1.0,
    };

    let multisample = VkPipelineMultisampleStateCreateInfo {
        s_type: VK_STRUCTURE_TYPE_PIPELINE_MULTISAMPLE_STATE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        rasterization_samples: VK_SAMPLE_COUNT_1_BIT,
        sample_shading_enable: VK_FALSE,
        min_sample_shading: 0.0,
        p_sample_mask: std::ptr::null(),
        alpha_to_coverage_enable: VK_FALSE,
        alpha_to_one_enable: VK_FALSE,
    };

    // Блендинга нет — как и в CPU-растеризаторе этого движка (см. «Залитые
    // пиксели всегда пишут альфу 255» в CLAUDE.md), пишем поверх без
    // смешивания
    let color_blend_attachment = VkPipelineColorBlendAttachmentState {
        blend_enable: VK_FALSE,
        src_color_blend_factor: 0,
        dst_color_blend_factor: 0,
        color_blend_op: 0,
        src_alpha_blend_factor: 0,
        dst_alpha_blend_factor: 0,
        alpha_blend_op: 0,
        color_write_mask: VK_COLOR_COMPONENT_R_BIT
            | VK_COLOR_COMPONENT_G_BIT
            | VK_COLOR_COMPONENT_B_BIT
            | VK_COLOR_COMPONENT_A_BIT,
    };

    let color_blend = VkPipelineColorBlendStateCreateInfo {
        s_type: VK_STRUCTURE_TYPE_PIPELINE_COLOR_BLEND_STATE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        logic_op_enable: VK_FALSE,
        logic_op: 0,
        attachment_count: 1,
        p_attachments: &color_blend_attachment,
        blend_constants: [0.0, 0.0, 0.0, 0.0],
    };

    let dynamic_states = [VK_DYNAMIC_STATE_VIEWPORT, VK_DYNAMIC_STATE_SCISSOR];
    let dynamic_state = VkPipelineDynamicStateCreateInfo {
        s_type: VK_STRUCTURE_TYPE_PIPELINE_DYNAMIC_STATE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        dynamic_state_count: dynamic_states.len() as u32,
        p_dynamic_states: dynamic_states.as_ptr(),
    };

    // Тест ВКЛЮЧЁН и ЗАПИСЬ ВКЛЮЧЕНА — оба нужны сразу нескольким
    // инстансам (Фаза 5): без записи второй инстанс не оставил бы следа в
    // буфере для проверки третьим, без теста заново включилась бы гонка
    // порядка отрисовки. `LESS`, а не `LESS_OR_EQUAL`: у двух инстансов
    // разная геометрия, и делить пиксель поровну им ни при каких условиях
    // не нужно (в отличие от двух треугольников ОДНОЙ грани на CPU-пути,
    // где как раз важно, что равная глубина не перезаписывается, см.
    // «При РАВНОЙ глубине выигрывает нарисованный раньше» в CLAUDE.md —
    // здесь эта тонкость не нужна, инстансы не делят вершины). Стенсила нет
    // вовсе — `stencil_test_enable: VK_FALSE`, поля `front`/`back` ни на что
    // не влияют, но обязаны нести синтаксически валидные значения
    let depth_stencil = VkPipelineDepthStencilStateCreateInfo {
        s_type: VK_STRUCTURE_TYPE_PIPELINE_DEPTH_STENCIL_STATE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        depth_test_enable: VK_TRUE,
        depth_write_enable: VK_TRUE,
        depth_compare_op: VK_COMPARE_OP_LESS,
        depth_bounds_test_enable: VK_FALSE,
        stencil_test_enable: VK_FALSE,
        front: VkStencilOpState {
            fail_op: VK_STENCIL_OP_KEEP,
            pass_op: VK_STENCIL_OP_KEEP,
            depth_fail_op: VK_STENCIL_OP_KEEP,
            compare_op: VK_COMPARE_OP_ALWAYS,
            compare_mask: 0,
            write_mask: 0,
            reference: 0,
        },
        back: VkStencilOpState {
            fail_op: VK_STENCIL_OP_KEEP,
            pass_op: VK_STENCIL_OP_KEEP,
            depth_fail_op: VK_STENCIL_OP_KEEP,
            compare_op: VK_COMPARE_OP_ALWAYS,
            compare_mask: 0,
            write_mask: 0,
            reference: 0,
        },
        min_depth_bounds: 0.0,
        max_depth_bounds: 1.0,
    };

    // MVP и матрица нормалей по-прежнему едут push-constant'ом — 112 байт
    // на кадр не стоят обвязки с descriptor pool/layout/set. Текстура
    // (Фаза 4) — ровно тот случай, для которого push-constant не годится
    // (картинка размером с текстуру, а не 112 байт), и вот там уже нужен
    // настоящий descriptor set, переданный сюда извне: сэмплер — это и
    // есть дескриптор. Гарантированный Vulkan'ом минимум для
    // `maxPushConstantsSize` — 128 байт у ЛЮБОЙ реализации (часть
    // спецификации, не особенность конкретного GPU), так что 112 байт
    // не рискуют упереться в лимит устройства без его запроса
    let push_constant_range = VkPushConstantRange {
        stage_flags: VK_SHADER_STAGE_VERTEX_BIT,
        offset: 0,
        size: std::mem::size_of::<PushConstants>() as u32,
    };
    let layout_info = VkPipelineLayoutCreateInfo {
        s_type: VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        set_layout_count: 1,
        p_set_layouts: &descriptor_set_layout,
        push_constant_range_count: 1,
        p_push_constant_ranges: &push_constant_range,
    };
    let mut layout = VkPipelineLayout::NULL;
    let result =
        unsafe { (device.fns.create_pipeline_layout)(device.handle, &layout_info, std::ptr::null(), &mut layout) };
    if result != VK_SUCCESS {
        return Err(format!("vkCreatePipelineLayout вернул {result}"));
    }

    let pipeline_info = VkGraphicsPipelineCreateInfo {
        s_type: VK_STRUCTURE_TYPE_GRAPHICS_PIPELINE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        stage_count: stages.len() as u32,
        p_stages: stages.as_ptr(),
        p_vertex_input_state: &vertex_input,
        p_input_assembly_state: &input_assembly,
        p_tessellation_state: std::ptr::null(),
        p_viewport_state: &viewport_state,
        p_rasterization_state: &rasterization,
        p_multisample_state: &multisample,
        p_depth_stencil_state: &depth_stencil,
        p_color_blend_state: &color_blend,
        p_dynamic_state: &dynamic_state,
        layout,
        render_pass,
        subpass: 0,
        base_pipeline_handle: VkPipeline::NULL,
        base_pipeline_index: -1,
    };

    let mut handle = VkPipeline::NULL;
    let result = unsafe {
        (device.fns.create_graphics_pipelines)(
            device.handle,
            crate::vulkan::ffi::VkPipelineCache::NULL,
            1,
            &pipeline_info,
            std::ptr::null(),
            &mut handle,
        )
    };
    if result != VK_SUCCESS {
        unsafe {
            (device.fns.destroy_pipeline_layout)(device.handle, layout, std::ptr::null());
        }
        return Err(format!("vkCreateGraphicsPipelines вернул {result}"));
    }

    Ok(Pipeline { layout, handle })
}
