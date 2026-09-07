//! Сырые C-совместимые типы Vulkan — только то подмножество API, которое
//! нужно Фазе 1 («hello triangle»): instance, device, surface, swapchain,
//! render pass, графический пайплайн, командные буферы, синхронизация.
//!
//! **Важно про происхождение чисел.** Числовые константы ниже (`sType`,
//! коды форматов, коды результатов) переписаны по памяти из спецификации
//! Vulkan — на этой машине не установлен Vulkan SDK, и сверить их с живым
//! `vulkan_core.h` было не с чем. Слой валидации (`VK_LAYER_KHRONOS_validation`,
//! см. `instance.rs`) — первая линия проверки: перепутанный `sType` или
//! неверный код структуры он обычно ловит с понятным сообщением вместо
//! молчаливого падения драйвера. Первый прогон после установки MoltenVK —
//! это и есть настоящая сверка.
//!
//! **Почему всё это не сгенерировано автоматически.** В реальных биндингах
//! (тот же `ash`) типы вычитывают из `vk.xml` кодогенератором — иначе на
//! тысячах структур и констант ошибка неизбежна. Здесь важнее учебная
//! ценность и следование философии проекта («самописно»), а не полнота: мы
//! заводим только те типы, которые реально используем, а не весь Vulkan.
//!
//! **Хендлы.** У Vulkan есть два вида хендлов — dispatchable (`VkInstance`,
//! `VkDevice`, `VkQueue`, `VkCommandBuffer`) и non-dispatchable (`VkSemaphore`,
//! `VkFence`, `VkImage`…). В оригинальном заголовке на 64-битных платформах
//! (macOS/Windows/Linux — всё, на что мы целимся) оба вида — это просто
//! непрозрачные указатели: `VK_USE_64_BIT_PTR_DEFINES=1` включает такое
//! определение для non-dispatchable хендлов ровно тогда же, когда указатель
//! на платформе 8 байт. На 32-битных платформах они были бы `u64`, но
//! настольные ОС 2020-х все 64-битные, и заводить отдельную ветку незачем.
//! Значит оба вида заводятся одним и тем же макросом.
//!
//! **`sType`/`pNext`.** Почти каждая `*CreateInfo`-структура начинается с
//! `sType` (тег, каким именно `Vk*CreateInfo` эта память является — Vulkan
//! не полагается на типы Rust, только на этот тег в рантайме) и `pNext`
//! (голова связного списка расширений структуры; мы расширений не
//! используем, поэтому `pNext` везде `null`).
//!
//! **`VkBool32` — это `u32`, а не `bool`.** Layout `bool` в Rust не
//! гарантирован совпадающим с C (хотя на практике это один байт), и что
//! важнее — Vulkan использует 4 байта (`VK_TRUE = 1`, `VK_FALSE = 0`), а не
//! один. Подставить туда Rust `bool` — значит доверить ABI языку, у
//! которого для этого нет контракта.

use std::ffi::c_void;
use std::os::raw::c_char;

// ============================================================================
// Базовые typedef'ы
// ============================================================================

pub type VkFlags = u32;
pub type VkBool32 = u32;
pub type VkDeviceSize = u64;
pub type VkSampleMask = u32;
/// `VkStructureType`, `VkFormat`, `VkResult` и подобные — в C это `enum`,
/// а размер `enum` без явного `enum class` в C — четыре байта со знаком
/// (`int`) на всех платформах, куда мы целимся. Поэтому заводим их как
/// `i32`, а не `u32`: у `VkResult` есть отрицательные коды ошибок.
pub type VkEnum = i32;

pub const VK_TRUE: VkBool32 = 1;
pub const VK_FALSE: VkBool32 = 0;

/// `VK_MAKE_API_VERSION(variant, major, minor, patch)` — кодирует версию
/// API в одно 32-битное число: 3 бита варианта, 7 бит major, 10 бит minor,
/// 12 бит patch. Нужно `VkApplicationInfo::apiVersion` и `VkInstanceCreateInfo`
pub const fn vk_make_api_version(variant: u32, major: u32, minor: u32, patch: u32) -> u32 {
    (variant << 29) | (major << 22) | (minor << 12) | patch
}
pub const VK_API_VERSION_1_0: u32 = vk_make_api_version(0, 1, 0, 0);

// ============================================================================
// VkResult — код возврата почти каждого вызова
// ============================================================================

pub const VK_SUCCESS: VkEnum = 0;
pub const VK_NOT_READY: VkEnum = 1;
pub const VK_TIMEOUT: VkEnum = 2;
pub const VK_SUBOPTIMAL_KHR: VkEnum = 1_000_001_003;
pub const VK_ERROR_OUT_OF_HOST_MEMORY: VkEnum = -1;
pub const VK_ERROR_OUT_OF_DEVICE_MEMORY: VkEnum = -2;
pub const VK_ERROR_INITIALIZATION_FAILED: VkEnum = -3;
pub const VK_ERROR_DEVICE_LOST: VkEnum = -4;
pub const VK_ERROR_LAYER_NOT_PRESENT: VkEnum = -6;
pub const VK_ERROR_EXTENSION_NOT_PRESENT: VkEnum = -7;
pub const VK_ERROR_INCOMPATIBLE_DRIVER: VkEnum = -9;
/// Swapchain пережил окно (resize, смена монитора) — сигнал пересобрать его,
/// не настоящая ошибка вызова
pub const VK_ERROR_OUT_OF_DATE_KHR: VkEnum = -1_000_001_004;
pub const VK_ERROR_SURFACE_LOST_KHR: VkEnum = -1_000_000_000;

// ============================================================================
// Хендлы
// ============================================================================

/// Заводит непрозрачный Vulkan-хендл: newtype над указателем с `NULL`-
/// константой для `VK_NULL_HANDLE`. Один макрос на оба вида хендлов —
/// почему, см. комментарий в начале файла
macro_rules! vk_handle {
    ($name:ident) => {
        #[repr(transparent)]
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub struct $name(pub *mut c_void);
        impl $name {
            pub const NULL: Self = Self(std::ptr::null_mut());
            pub fn is_null(self) -> bool {
                self.0.is_null()
            }
        }
    };
}

vk_handle!(VkInstance);
vk_handle!(VkPhysicalDevice);
vk_handle!(VkDevice);
vk_handle!(VkQueue);
vk_handle!(VkCommandBuffer);
vk_handle!(VkSurfaceKHR);
vk_handle!(VkSwapchainKHR);
vk_handle!(VkImage);
vk_handle!(VkImageView);
vk_handle!(VkRenderPass);
vk_handle!(VkFramebuffer);
vk_handle!(VkShaderModule);
vk_handle!(VkPipelineLayout);
vk_handle!(VkPipeline);
vk_handle!(VkCommandPool);
vk_handle!(VkSemaphore);
vk_handle!(VkFence);
vk_handle!(VkBuffer);
vk_handle!(VkDeviceMemory);
// `VkDescriptorSetLayout` заведён был ещё в Фазе 1 — только ради сигнатуры
// `VkPipelineLayoutCreateInfo::pSetLayouts`, самих дескрипторов тогда не
// было; с Фазы 4 создаётся по-настоящему (`descriptor::create_set_layout`)
vk_handle!(VkDescriptorSetLayout);
vk_handle!(VkSampler);
vk_handle!(VkDescriptorPool);
vk_handle!(VkDescriptorSet);
// Нигде не создаётся — `vkCreateGraphicsPipelines` вторым параметром хочет
// кеш пайплайнов, мы всегда передаём `NULL`, но тип параметра должен быть
// настоящим, а не позаимствованным у `VkPipeline`.
//
// Здесь и выше — обычные `//`, а не `///`: `vk_handle!` — макрос, и rustdoc
// к его раскрытию doc-комментарий не прикрепляет (компилятор ругается
// `unused doc comment`). Пояснение нужно читателю файла, а не rustdoc
vk_handle!(VkPipelineCache);

// ============================================================================
// Геометрические вспомогательные структуры
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VkExtent2D {
    pub width: u32,
    pub height: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VkExtent3D {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VkOffset2D {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VkRect2D {
    pub offset: VkOffset2D,
    pub extent: VkExtent2D,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VkViewport {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub min_depth: f32,
    pub max_depth: f32,
}

// ============================================================================
// sType — тег структуры (см. объяснение в начале файла)
// ============================================================================

pub const VK_STRUCTURE_TYPE_APPLICATION_INFO: VkEnum = 0;
pub const VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO: VkEnum = 1;
pub const VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO: VkEnum = 2;
pub const VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO: VkEnum = 3;
pub const VK_STRUCTURE_TYPE_SUBMIT_INFO: VkEnum = 4;
pub const VK_STRUCTURE_TYPE_FENCE_CREATE_INFO: VkEnum = 8;
pub const VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO: VkEnum = 9;
pub const VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO: VkEnum = 16;
pub const VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO: VkEnum = 18;
pub const VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO: VkEnum = 19;
pub const VK_STRUCTURE_TYPE_PIPELINE_INPUT_ASSEMBLY_STATE_CREATE_INFO: VkEnum = 20;
pub const VK_STRUCTURE_TYPE_PIPELINE_VIEWPORT_STATE_CREATE_INFO: VkEnum = 22;
pub const VK_STRUCTURE_TYPE_PIPELINE_RASTERIZATION_STATE_CREATE_INFO: VkEnum = 23;
pub const VK_STRUCTURE_TYPE_PIPELINE_MULTISAMPLE_STATE_CREATE_INFO: VkEnum = 24;
pub const VK_STRUCTURE_TYPE_PIPELINE_COLOR_BLEND_STATE_CREATE_INFO: VkEnum = 26;
pub const VK_STRUCTURE_TYPE_PIPELINE_DYNAMIC_STATE_CREATE_INFO: VkEnum = 27;
pub const VK_STRUCTURE_TYPE_GRAPHICS_PIPELINE_CREATE_INFO: VkEnum = 28;
pub const VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO: VkEnum = 30;
pub const VK_STRUCTURE_TYPE_FRAMEBUFFER_CREATE_INFO: VkEnum = 37;
pub const VK_STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO: VkEnum = 38;
pub const VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO: VkEnum = 39;
pub const VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO: VkEnum = 40;
pub const VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO: VkEnum = 42;
pub const VK_STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO: VkEnum = 43;
pub const VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO: VkEnum = 15;
pub const VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO: VkEnum = 5;
pub const VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO: VkEnum = 12;
pub const VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO: VkEnum = 14;
pub const VK_STRUCTURE_TYPE_SAMPLER_CREATE_INFO: VkEnum = 31;
pub const VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO: VkEnum = 32;
pub const VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO: VkEnum = 33;
pub const VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO: VkEnum = 34;
pub const VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET: VkEnum = 35;
pub const VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER: VkEnum = 45;

/// Значения из расширений (KHR/EXT) кодируются не подряд, а по формуле
/// `1_000_000_000 + (номер_расширения_в_реестре - 1) * 1000 + смещение`.
/// `VK_KHR_swapchain` — расширение №2, отсюда `1_000_001_xxx`
pub const VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR: VkEnum = 1_000_001_000;
pub const VK_STRUCTURE_TYPE_PRESENT_INFO_KHR: VkEnum = 1_000_001_001;
/// `VK_KHR_win32_surface` — расширение №10
pub const VK_STRUCTURE_TYPE_WIN32_SURFACE_CREATE_INFO_KHR: VkEnum = 1_000_009_000;
/// `VK_KHR_xlib_surface` — расширение №5
pub const VK_STRUCTURE_TYPE_XLIB_SURFACE_CREATE_INFO_KHR: VkEnum = 1_000_004_000;
/// `VK_EXT_metal_surface` — расширение №218, добавлено вместе с MoltenVK
pub const VK_STRUCTURE_TYPE_METAL_SURFACE_CREATE_INFO_EXT: VkEnum = 1_000_217_000;

// ============================================================================
// Instance
// ============================================================================

#[repr(C)]
pub struct VkApplicationInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub p_application_name: *const c_char,
    pub application_version: u32,
    pub p_engine_name: *const c_char,
    pub engine_version: u32,
    pub api_version: u32,
}

#[repr(C)]
pub struct VkInstanceCreateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub p_application_info: *const VkApplicationInfo,
    pub enabled_layer_count: u32,
    pub pp_enabled_layer_names: *const *const c_char,
    pub enabled_extension_count: u32,
    pub pp_enabled_extension_names: *const *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VkLayerProperties {
    pub layer_name: [c_char; 256],
    pub spec_version: u32,
    pub implementation_version: u32,
    pub description: [c_char; 256],
}

/// Одно расширение из списка `vkEnumerateInstanceExtensionProperties` или
/// `vkEnumerateDeviceExtensionProperties`. Имя — фиксированный массив байт с
/// нулём внутри, а не указатель: список отдаётся сразу целиком, и владеть
/// строками отдельно Vulkan не хочет
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VkExtensionProperties {
    pub extension_name: [c_char; 256],
    pub spec_version: u32,
}

/// Флаг `VkInstanceCreateInfo::flags`: «перечисляй и НЕполноценные
/// реализации Vulkan». Без него Vulkan Loader делает вид, что таких
/// драйверов нет вовсе, — а MoltenVK ровно такой (см. `Instance::new`)
pub const VK_INSTANCE_CREATE_ENUMERATE_PORTABILITY_BIT_KHR: VkFlags = 0x0000_0001;

// ============================================================================
// Physical device / очереди
// ============================================================================

pub const VK_QUEUE_GRAPHICS_BIT: VkFlags = 0x0000_0001;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VkQueueFamilyProperties {
    pub queue_flags: VkFlags,
    pub queue_count: u32,
    pub timestamp_valid_bits: u32,
    pub min_image_transfer_granularity: VkExtent3D,
}

// ============================================================================
// Logical device
// ============================================================================

#[repr(C)]
pub struct VkDeviceQueueCreateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub queue_family_index: u32,
    pub queue_count: u32,
    pub p_queue_priorities: *const f32,
}

#[repr(C)]
pub struct VkDeviceCreateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub queue_create_info_count: u32,
    pub p_queue_create_infos: *const VkDeviceQueueCreateInfo,
    pub enabled_layer_count: u32,
    pub pp_enabled_layer_names: *const *const c_char,
    pub enabled_extension_count: u32,
    pub pp_enabled_extension_names: *const *const c_char,
    /// Всегда `null` в Фазе 1 — мы не запрашиваем ни одной необязательной
    /// физической фичи (анизотропию для сэмплера и т.п. попросим отдельно,
    /// когда дойдём до текстур в Фазе 4)
    pub p_enabled_features: *const c_void,
}

// ============================================================================
// Surface (VK_KHR_surface — общая часть, платформенные CreateInfo в surface.rs)
// ============================================================================

pub const VK_FORMAT_UNDEFINED: VkEnum = 0;
pub const VK_FORMAT_B8G8R8A8_UNORM: VkEnum = 44;
pub const VK_COLOR_SPACE_SRGB_NONLINEAR_KHR: VkEnum = 0;

pub const VK_PRESENT_MODE_FIFO_KHR: VkEnum = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VkSurfaceFormatKHR {
    pub format: VkEnum,
    pub color_space: VkEnum,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VkSurfaceCapabilitiesKHR {
    pub min_image_count: u32,
    pub max_image_count: u32,
    pub current_extent: VkExtent2D,
    pub min_image_extent: VkExtent2D,
    pub max_image_extent: VkExtent2D,
    pub max_image_array_layers: u32,
    pub supported_transforms: VkFlags,
    pub current_transform: VkFlags,
    pub supported_composite_alpha: VkFlags,
    pub supported_usage_flags: VkFlags,
}

pub const VK_SURFACE_TRANSFORM_IDENTITY_BIT_KHR: VkFlags = 0x0000_0001;
pub const VK_COMPOSITE_ALPHA_OPAQUE_BIT_KHR: VkFlags = 0x0000_0001;
pub const VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT: VkFlags = 0x0000_0010;
pub const VK_SHARING_MODE_EXCLUSIVE: VkEnum = 0;

// ============================================================================
// Swapchain
// ============================================================================

#[repr(C)]
pub struct VkSwapchainCreateInfoKHR {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub surface: VkSurfaceKHR,
    pub min_image_count: u32,
    pub image_format: VkEnum,
    pub image_color_space: VkEnum,
    pub image_extent: VkExtent2D,
    pub image_array_layers: u32,
    pub image_usage: VkFlags,
    pub image_sharing_mode: VkEnum,
    pub queue_family_index_count: u32,
    pub p_queue_family_indices: *const u32,
    pub pre_transform: VkFlags,
    pub composite_alpha: VkFlags,
    pub present_mode: VkEnum,
    pub clipped: VkBool32,
    pub old_swapchain: VkSwapchainKHR,
}

// ============================================================================
// Image view
// ============================================================================

pub const VK_IMAGE_VIEW_TYPE_2D: VkEnum = 1;
pub const VK_COMPONENT_SWIZZLE_IDENTITY: VkEnum = 0;
pub const VK_IMAGE_ASPECT_COLOR_BIT: VkFlags = 0x0000_0001;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VkComponentMapping {
    pub r: VkEnum,
    pub g: VkEnum,
    pub b: VkEnum,
    pub a: VkEnum,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VkImageSubresourceRange {
    pub aspect_mask: VkFlags,
    pub base_mip_level: u32,
    pub level_count: u32,
    pub base_array_layer: u32,
    pub layer_count: u32,
}

#[repr(C)]
pub struct VkImageViewCreateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub image: VkImage,
    pub view_type: VkEnum,
    pub format: VkEnum,
    pub components: VkComponentMapping,
    pub subresource_range: VkImageSubresourceRange,
}

// ============================================================================
// Render pass
// ============================================================================

pub const VK_SAMPLE_COUNT_1_BIT: VkFlags = 0x0000_0001;
pub const VK_ATTACHMENT_LOAD_OP_LOAD: VkEnum = 0;
pub const VK_ATTACHMENT_LOAD_OP_CLEAR: VkEnum = 1;
pub const VK_ATTACHMENT_LOAD_OP_DONT_CARE: VkEnum = 2;
pub const VK_ATTACHMENT_STORE_OP_STORE: VkEnum = 0;
pub const VK_ATTACHMENT_STORE_OP_DONT_CARE: VkEnum = 1;
pub const VK_IMAGE_LAYOUT_UNDEFINED: VkEnum = 0;
pub const VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL: VkEnum = 2;
pub const VK_IMAGE_LAYOUT_PRESENT_SRC_KHR: VkEnum = 1_000_001_002;
pub const VK_PIPELINE_BIND_POINT_GRAPHICS: VkEnum = 0;
/// Команды подпасса пишутся прямо в основной командный буфер, а не в
/// отдельные вторичные — у нас их и нет
pub const VK_SUBPASS_CONTENTS_INLINE: VkEnum = 0;
/// Означает «не подпасс, а всё, что снаружи render pass» в `VkSubpassDependency`
pub const VK_SUBPASS_EXTERNAL: u32 = u32::MAX;
pub const VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT: VkFlags = 0x0000_0400;
pub const VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT: VkFlags = 0x0000_0100;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VkAttachmentDescription {
    pub flags: VkFlags,
    pub format: VkEnum,
    pub samples: VkFlags,
    pub load_op: VkEnum,
    pub store_op: VkEnum,
    pub stencil_load_op: VkEnum,
    pub stencil_store_op: VkEnum,
    pub initial_layout: VkEnum,
    pub final_layout: VkEnum,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VkAttachmentReference {
    pub attachment: u32,
    pub layout: VkEnum,
}

#[repr(C)]
pub struct VkSubpassDescription {
    pub flags: VkFlags,
    pub pipeline_bind_point: VkEnum,
    pub input_attachment_count: u32,
    pub p_input_attachments: *const VkAttachmentReference,
    pub color_attachment_count: u32,
    pub p_color_attachments: *const VkAttachmentReference,
    pub p_resolve_attachments: *const VkAttachmentReference,
    pub p_depth_stencil_attachment: *const VkAttachmentReference,
    pub preserve_attachment_count: u32,
    pub p_preserve_attachments: *const u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VkSubpassDependency {
    pub src_subpass: u32,
    pub dst_subpass: u32,
    pub src_stage_mask: VkFlags,
    pub dst_stage_mask: VkFlags,
    pub src_access_mask: VkFlags,
    pub dst_access_mask: VkFlags,
    pub dependency_flags: VkFlags,
}

#[repr(C)]
pub struct VkRenderPassCreateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub attachment_count: u32,
    pub p_attachments: *const VkAttachmentDescription,
    pub subpass_count: u32,
    pub p_subpasses: *const VkSubpassDescription,
    pub dependency_count: u32,
    pub p_dependencies: *const VkSubpassDependency,
}

// ============================================================================
// Шейдерные модули и пайплайн
// ============================================================================

pub const VK_SHADER_STAGE_VERTEX_BIT: VkFlags = 0x0000_0001;
pub const VK_SHADER_STAGE_FRAGMENT_BIT: VkFlags = 0x0000_0010;
pub const VK_PRIMITIVE_TOPOLOGY_TRIANGLE_LIST: VkEnum = 3;
pub const VK_POLYGON_MODE_FILL: VkEnum = 0;
pub const VK_CULL_MODE_NONE: VkFlags = 0;
pub const VK_CULL_MODE_BACK_BIT: VkFlags = 0x0000_0002;
/// Против часовой стрелки при взгляде снаружи — та же конвенция обхода,
/// что и у CPU-растеризатора (см. `is_backface` в `renderer/triangle.rs`)
pub const VK_FRONT_FACE_COUNTER_CLOCKWISE: VkEnum = 0;
pub const VK_COLOR_COMPONENT_R_BIT: VkFlags = 0x1;
pub const VK_COLOR_COMPONENT_G_BIT: VkFlags = 0x2;
pub const VK_COLOR_COMPONENT_B_BIT: VkFlags = 0x4;
pub const VK_COLOR_COMPONENT_A_BIT: VkFlags = 0x8;
pub const VK_DYNAMIC_STATE_VIEWPORT: VkEnum = 0;
pub const VK_DYNAMIC_STATE_SCISSOR: VkEnum = 1;

#[repr(C)]
pub struct VkShaderModuleCreateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    /// В БАЙТАХ, не в словах — `code.len() * 4`, а не `code.len()`
    pub code_size: usize,
    pub p_code: *const u32,
}

#[repr(C)]
pub struct VkPipelineShaderStageCreateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub stage: VkFlags,
    pub module: VkShaderModule,
    pub p_name: *const c_char,
    pub p_specialization_info: *const c_void,
}

#[repr(C)]
pub struct VkPipelineVertexInputStateCreateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub vertex_binding_description_count: u32,
    pub p_vertex_binding_descriptions: *const VkVertexInputBindingDescription,
    pub vertex_attribute_description_count: u32,
    pub p_vertex_attribute_descriptions: *const VkVertexInputAttributeDescription,
}

#[repr(C)]
pub struct VkPipelineInputAssemblyStateCreateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub topology: VkEnum,
    pub primitive_restart_enable: VkBool32,
}

#[repr(C)]
pub struct VkPipelineViewportStateCreateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub viewport_count: u32,
    pub p_viewports: *const VkViewport,
    pub scissor_count: u32,
    pub p_scissors: *const VkRect2D,
}

#[repr(C)]
pub struct VkPipelineRasterizationStateCreateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub depth_clamp_enable: VkBool32,
    pub rasterizer_discard_enable: VkBool32,
    pub polygon_mode: VkEnum,
    pub cull_mode: VkFlags,
    pub front_face: VkEnum,
    pub depth_bias_enable: VkBool32,
    pub depth_bias_constant_factor: f32,
    pub depth_bias_clamp: f32,
    pub depth_bias_slope_factor: f32,
    pub line_width: f32,
}

#[repr(C)]
pub struct VkPipelineMultisampleStateCreateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub rasterization_samples: VkFlags,
    pub sample_shading_enable: VkBool32,
    pub min_sample_shading: f32,
    pub p_sample_mask: *const VkSampleMask,
    pub alpha_to_coverage_enable: VkBool32,
    pub alpha_to_one_enable: VkBool32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VkPipelineColorBlendAttachmentState {
    pub blend_enable: VkBool32,
    pub src_color_blend_factor: VkEnum,
    pub dst_color_blend_factor: VkEnum,
    pub color_blend_op: VkEnum,
    pub src_alpha_blend_factor: VkEnum,
    pub dst_alpha_blend_factor: VkEnum,
    pub alpha_blend_op: VkEnum,
    pub color_write_mask: VkFlags,
}

#[repr(C)]
pub struct VkPipelineColorBlendStateCreateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub logic_op_enable: VkBool32,
    pub logic_op: VkEnum,
    pub attachment_count: u32,
    pub p_attachments: *const VkPipelineColorBlendAttachmentState,
    pub blend_constants: [f32; 4],
}

#[repr(C)]
pub struct VkPipelineDynamicStateCreateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub dynamic_state_count: u32,
    pub p_dynamic_states: *const VkEnum,
}

#[repr(C)]
pub struct VkPipelineLayoutCreateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub set_layout_count: u32,
    pub p_set_layouts: *const VkDescriptorSetLayout,
    pub push_constant_range_count: u32,
    pub p_push_constant_ranges: *const VkPushConstantRange,
}

#[repr(C)]
pub struct VkGraphicsPipelineCreateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub stage_count: u32,
    pub p_stages: *const VkPipelineShaderStageCreateInfo,
    pub p_vertex_input_state: *const VkPipelineVertexInputStateCreateInfo,
    pub p_input_assembly_state: *const VkPipelineInputAssemblyStateCreateInfo,
    pub p_tessellation_state: *const c_void,
    pub p_viewport_state: *const VkPipelineViewportStateCreateInfo,
    pub p_rasterization_state: *const VkPipelineRasterizationStateCreateInfo,
    pub p_multisample_state: *const VkPipelineMultisampleStateCreateInfo,
    pub p_depth_stencil_state: *const VkPipelineDepthStencilStateCreateInfo,
    pub p_color_blend_state: *const VkPipelineColorBlendStateCreateInfo,
    pub p_dynamic_state: *const VkPipelineDynamicStateCreateInfo,
    pub layout: VkPipelineLayout,
    pub render_pass: VkRenderPass,
    pub subpass: u32,
    pub base_pipeline_handle: VkPipeline,
    pub base_pipeline_index: i32,
}

// ============================================================================
// Framebuffer, командные пулы/буферы
// ============================================================================

pub const VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT: VkFlags = 0x0000_0002;
pub const VK_COMMAND_BUFFER_LEVEL_PRIMARY: VkEnum = 0;

#[repr(C)]
pub struct VkFramebufferCreateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub render_pass: VkRenderPass,
    pub attachment_count: u32,
    pub p_attachments: *const VkImageView,
    pub width: u32,
    pub height: u32,
    pub layers: u32,
}

#[repr(C)]
pub struct VkCommandPoolCreateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub queue_family_index: u32,
}

#[repr(C)]
pub struct VkCommandBufferAllocateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub command_pool: VkCommandPool,
    pub level: VkEnum,
    pub command_buffer_count: u32,
}

#[repr(C)]
pub struct VkCommandBufferBeginInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub p_inheritance_info: *const c_void,
}

/// Соответствует `VkClearColorValue` (union в C, но все варианты — 16 байт,
/// а мы всегда пишем/читаем его как `float32`, так что достаточно одного
/// поля той же ширины, что и весь union)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VkClearColorValue {
    pub float32: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VkClearDepthStencilValue {
    pub depth: f32,
    pub stencil: u32,
}

/// `VkClearValue` в C — union (цвет ИЛИ глубина/трафарет). Фаза 1-4
/// обходились одним полем `color` (глубины не было вовсе), но Фаза 5
/// добавляет depth attachment со СВОИМ clear-значением в тот же массив
/// `p_clear_values` — а раз оба варианта в одном массиве бок о бок, они
/// обязаны делить память по-настоящему: обычная Rust-структура с двумя
/// полями заняла бы 16+8=24 байта на элемент вместо 16, и массив
/// рассыпался бы на несовпадающие по размеру блоки. Настоящий `union` —
/// ЕДИНСТВЕННЫЙ способ получить точно ту раскладку, которую ждёт Vulkan
#[repr(C)]
#[derive(Clone, Copy)]
pub union VkClearValue {
    pub color: VkClearColorValue,
    pub depth_stencil: VkClearDepthStencilValue,
}

#[repr(C)]
pub struct VkRenderPassBeginInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub render_pass: VkRenderPass,
    pub framebuffer: VkFramebuffer,
    pub render_area: VkRect2D,
    pub clear_value_count: u32,
    pub p_clear_values: *const VkClearValue,
}

// ============================================================================
// Синхронизация
// ============================================================================

pub const VK_FENCE_CREATE_SIGNALED_BIT: VkFlags = 0x0000_0001;

#[repr(C)]
pub struct VkSemaphoreCreateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
}

#[repr(C)]
pub struct VkFenceCreateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
}

#[repr(C)]
pub struct VkSubmitInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub wait_semaphore_count: u32,
    pub p_wait_semaphores: *const VkSemaphore,
    pub p_wait_dst_stage_mask: *const VkFlags,
    pub command_buffer_count: u32,
    pub p_command_buffers: *const VkCommandBuffer,
    pub signal_semaphore_count: u32,
    pub p_signal_semaphores: *const VkSemaphore,
}

// ============================================================================
// Память и буферы (Фаза 2)
// ============================================================================

pub const VK_MAX_MEMORY_TYPES: usize = 32;
pub const VK_MAX_MEMORY_HEAPS: usize = 16;

pub const VK_BUFFER_USAGE_VERTEX_BUFFER_BIT: VkFlags = 0x0000_0080;
pub const VK_BUFFER_USAGE_INDEX_BUFFER_BIT: VkFlags = 0x0000_0040;

pub const VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT: VkFlags = 0x0000_0002;
pub const VK_MEMORY_PROPERTY_HOST_COHERENT_BIT: VkFlags = 0x0000_0004;

pub const VK_FORMAT_R32G32B32_SFLOAT: VkEnum = 106;
pub const VK_VERTEX_INPUT_RATE_VERTEX: VkEnum = 0;
pub const VK_INDEX_TYPE_UINT32: VkEnum = 1;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VkMemoryType {
    pub property_flags: VkFlags,
    pub heap_index: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VkMemoryHeap {
    pub size: VkDeviceSize,
    pub flags: VkFlags,
}

/// Массивы фиксированного размера — так их и объявляет сам Vulkan
/// (`VK_MAX_MEMORY_TYPES`/`VK_MAX_MEMORY_HEAPS`), а не `Vec`: структуру
/// заполняет `vkGetPhysicalDeviceMemoryProperties` одним вызовом без
/// двухшstep-запроса «сколько-потом-давай», который нужен спискам
/// переменной длины вроде форматов поверхности
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkPhysicalDeviceMemoryProperties {
    pub memory_type_count: u32,
    pub memory_types: [VkMemoryType; VK_MAX_MEMORY_TYPES],
    pub memory_heap_count: u32,
    pub memory_heaps: [VkMemoryHeap; VK_MAX_MEMORY_HEAPS],
}

#[repr(C)]
pub struct VkBufferCreateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub size: VkDeviceSize,
    pub usage: VkFlags,
    pub sharing_mode: VkEnum,
    pub queue_family_index_count: u32,
    pub p_queue_family_indices: *const u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VkMemoryRequirements {
    pub size: VkDeviceSize,
    pub alignment: VkDeviceSize,
    pub memory_type_bits: u32,
}

#[repr(C)]
pub struct VkMemoryAllocateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub allocation_size: VkDeviceSize,
    pub memory_type_index: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VkVertexInputBindingDescription {
    pub binding: u32,
    pub stride: u32,
    pub input_rate: VkEnum,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VkVertexInputAttributeDescription {
    pub location: u32,
    pub binding: u32,
    pub format: VkEnum,
    pub offset: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VkPushConstantRange {
    pub stage_flags: VkFlags,
    pub offset: u32,
    pub size: u32,
}

#[repr(C)]
pub struct VkPresentInfoKHR {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub wait_semaphore_count: u32,
    pub p_wait_semaphores: *const VkSemaphore,
    pub swapchain_count: u32,
    pub p_swapchains: *const VkSwapchainKHR,
    pub p_image_indices: *const u32,
    pub p_results: *mut VkEnum,
}

// ============================================================================
// Изображения и сэмплер (Фаза 4)
// ============================================================================

pub const VK_IMAGE_TYPE_2D: VkEnum = 1;
pub const VK_IMAGE_TILING_OPTIMAL: VkEnum = 0;
pub const VK_IMAGE_USAGE_TRANSFER_SRC_BIT: VkFlags = 0x0000_0001;
pub const VK_IMAGE_USAGE_TRANSFER_DST_BIT: VkFlags = 0x0000_0002;
pub const VK_IMAGE_USAGE_SAMPLED_BIT: VkFlags = 0x0000_0004;
pub const VK_BUFFER_USAGE_TRANSFER_SRC_BIT: VkFlags = 0x0000_0001;
pub const VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT: VkFlags = 0x0000_0001;
pub const VK_FORMAT_R8G8B8A8_UNORM: VkEnum = 37;
pub const VK_FORMAT_R32G32_SFLOAT: VkEnum = 103;

pub const VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL: VkEnum = 5;
pub const VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL: VkEnum = 7;

pub const VK_ACCESS_SHADER_READ_BIT: VkFlags = 0x0000_0020;
pub const VK_ACCESS_TRANSFER_WRITE_BIT: VkFlags = 0x0000_1000;
pub const VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT: VkFlags = 0x0000_0001;
pub const VK_PIPELINE_STAGE_FRAGMENT_SHADER_BIT: VkFlags = 0x0000_0080;
pub const VK_PIPELINE_STAGE_TRANSFER_BIT: VkFlags = 0x0000_1000;
/// «Не переносить владение между семействами очередей» — у нас одна очередь
/// на графику и презентацию (см. `device::pick_physical_device`), поэтому
/// каждый барьер здесь ставит его в обоих полях. Значение то же самое, что
/// у `VK_SUBPASS_EXTERNAL` (`u32::MAX`), но это отдельная константа своего
/// смысла, а не переиспользование той
pub const VK_QUEUE_FAMILY_IGNORED: u32 = u32::MAX;
pub const VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT: VkFlags = 0x0000_0001;

pub const VK_FILTER_NEAREST: VkEnum = 0;
pub const VK_FILTER_LINEAR: VkEnum = 1;
pub const VK_SAMPLER_MIPMAP_MODE_NEAREST: VkEnum = 0;
pub const VK_SAMPLER_MIPMAP_MODE_LINEAR: VkEnum = 1;
/// Заворачивает, а не зажимает — та же конвенция, что у `Texture::sample`
/// на CPU-пути (см. «Выборка текселя заворачивает, а не зажимает» в
/// CLAUDE.md)
pub const VK_SAMPLER_ADDRESS_MODE_REPEAT: VkEnum = 0;
pub const VK_COMPARE_OP_ALWAYS: VkEnum = 7;
/// Не используется на практике: он только для `CLAMP_TO_BORDER`, а у нас
/// везде `REPEAT`. Нужен просто как синтаксически валидное значение поля
pub const VK_BORDER_COLOR_INT_OPAQUE_BLACK: VkEnum = 3;

pub const VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER: VkEnum = 1;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VkOffset3D {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[repr(C)]
pub struct VkImageCreateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub image_type: VkEnum,
    pub format: VkEnum,
    pub extent: VkExtent3D,
    pub mip_levels: u32,
    pub array_layers: u32,
    pub samples: VkFlags,
    pub tiling: VkEnum,
    pub usage: VkFlags,
    pub sharing_mode: VkEnum,
    pub queue_family_index_count: u32,
    pub p_queue_family_indices: *const u32,
    pub initial_layout: VkEnum,
}

#[repr(C)]
pub struct VkImageMemoryBarrier {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub src_access_mask: VkFlags,
    pub dst_access_mask: VkFlags,
    pub old_layout: VkEnum,
    pub new_layout: VkEnum,
    pub src_queue_family_index: u32,
    pub dst_queue_family_index: u32,
    pub image: VkImage,
    pub subresource_range: VkImageSubresourceRange,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VkImageSubresourceLayers {
    pub aspect_mask: VkFlags,
    pub mip_level: u32,
    pub base_array_layer: u32,
    pub layer_count: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VkBufferImageCopy {
    pub buffer_offset: VkDeviceSize,
    /// 0 — тесно упаковано (тексели идут подряд, как и лежат в
    /// `Texture::level0_rgba8`), а не настоящая ширина строки
    pub buffer_row_length: u32,
    pub buffer_image_height: u32,
    pub image_subresource: VkImageSubresourceLayers,
    pub image_offset: VkOffset3D,
    pub image_extent: VkExtent3D,
}

#[repr(C)]
pub struct VkSamplerCreateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub mag_filter: VkEnum,
    pub min_filter: VkEnum,
    pub mipmap_mode: VkEnum,
    pub address_mode_u: VkEnum,
    pub address_mode_v: VkEnum,
    pub address_mode_w: VkEnum,
    pub mip_lod_bias: f32,
    pub anisotropy_enable: VkBool32,
    pub max_anisotropy: f32,
    pub compare_enable: VkBool32,
    pub compare_op: VkEnum,
    pub min_lod: f32,
    pub max_lod: f32,
    pub border_color: VkEnum,
    pub unnormalized_coordinates: VkBool32,
}

// ============================================================================
// Descriptor set'ы (Фаза 4) — впервые в этом движке: до сих пор вся
// динамика шла через push-constant, а картинка размером с текстуру туда не
// влезает (и не должна — push-constant гарантированно доступен только
// на 128 байт, см. `pipeline::PushConstants`)
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VkDescriptorSetLayoutBinding {
    pub binding: u32,
    pub descriptor_type: VkEnum,
    pub descriptor_count: u32,
    pub stage_flags: VkFlags,
    pub p_immutable_samplers: *const VkSampler,
}

#[repr(C)]
pub struct VkDescriptorSetLayoutCreateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub binding_count: u32,
    pub p_bindings: *const VkDescriptorSetLayoutBinding,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VkDescriptorPoolSize {
    /// В спецификации поле называется `type` — зарезервированное слово в
    /// Rust. На раскладке структуры имя не сказывается: она идёт по
    /// порядку полей, а не по имени
    pub descriptor_type: VkEnum,
    pub descriptor_count: u32,
}

#[repr(C)]
pub struct VkDescriptorPoolCreateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub max_sets: u32,
    pub pool_size_count: u32,
    pub p_pool_sizes: *const VkDescriptorPoolSize,
}

#[repr(C)]
pub struct VkDescriptorSetAllocateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub descriptor_pool: VkDescriptorPool,
    pub descriptor_set_count: u32,
    pub p_set_layouts: *const VkDescriptorSetLayout,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VkDescriptorImageInfo {
    pub sampler: VkSampler,
    pub image_view: VkImageView,
    pub image_layout: VkEnum,
}

#[repr(C)]
pub struct VkWriteDescriptorSet {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub dst_set: VkDescriptorSet,
    pub dst_binding: u32,
    pub dst_array_element: u32,
    pub descriptor_count: u32,
    pub descriptor_type: VkEnum,
    pub p_image_info: *const VkDescriptorImageInfo,
    pub p_buffer_info: *const c_void,
    pub p_texel_buffer_view: *const c_void,
}

// ============================================================================
// Depth buffer (Фаза 5)
// ============================================================================

/// Гарантированно поддерживается как depth attachment на ЛЮБОЙ реализации
/// Vulkan (часть спецификации, не особенность конкретного GPU) — в отличие
/// от `D24_UNORM_S8_UINT`, чья поддержка формально опциональна. Стенсил нам
/// не нужен вовсе, поэтому берём чистый формат глубины, а не связку
pub const VK_FORMAT_D32_SFLOAT: VkEnum = 126;
pub const VK_IMAGE_ASPECT_DEPTH_BIT: VkFlags = 0x0000_0002;
pub const VK_IMAGE_USAGE_DEPTH_STENCIL_ATTACHMENT_BIT: VkFlags = 0x0000_0020;
pub const VK_IMAGE_LAYOUT_DEPTH_STENCIL_ATTACHMENT_OPTIMAL: VkEnum = 3;
pub const VK_PIPELINE_STAGE_EARLY_FRAGMENT_TESTS_BIT: VkFlags = 0x0000_0100;
pub const VK_PIPELINE_STAGE_LATE_FRAGMENT_TESTS_BIT: VkFlags = 0x0000_0200;
pub const VK_ACCESS_DEPTH_STENCIL_ATTACHMENT_WRITE_BIT: VkFlags = 0x0000_0400;
/// Меньшее значение — ближе (стандартная конвенция «меньше побеждает» для
/// [0,1]-глубины Vulkan; не путать с этим же движком на CPU, где в
/// `depth_buffer.rs` буфер хранит `1/w` и там ПОБЕЖДАЕТ БОЛЬШЕЕ — конвенции
/// разных буферов не обязаны совпадать, лишь бы тест и запись были
/// согласованы друг с другом внутри одного буфера)
pub const VK_COMPARE_OP_LESS: VkEnum = 1;
/// Не используется по-настоящему (стенсила нет), но поле обязано нести
/// синтаксически валидное значение
pub const VK_STENCIL_OP_KEEP: VkEnum = 0;
pub const VK_STRUCTURE_TYPE_PIPELINE_DEPTH_STENCIL_STATE_CREATE_INFO: VkEnum = 25;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VkStencilOpState {
    pub fail_op: VkEnum,
    pub pass_op: VkEnum,
    pub depth_fail_op: VkEnum,
    pub compare_op: VkEnum,
    pub compare_mask: u32,
    pub write_mask: u32,
    pub reference: u32,
}

#[repr(C)]
pub struct VkPipelineDepthStencilStateCreateInfo {
    pub s_type: VkEnum,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub depth_test_enable: VkBool32,
    pub depth_write_enable: VkBool32,
    pub depth_compare_op: VkEnum,
    pub depth_bounds_test_enable: VkBool32,
    pub stencil_test_enable: VkBool32,
    pub front: VkStencilOpState,
    pub back: VkStencilOpState,
    pub min_depth_bounds: f32,
    pub max_depth_bounds: f32,
}
