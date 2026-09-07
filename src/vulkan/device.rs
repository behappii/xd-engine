//! Физическое устройство (какая видеокарта), логическое устройство (наш
//! канал к ней) и таблица device-level функций — вообще всё, что дальше
//! принимает `VkDevice`, `VkQueue` или `VkCommandBuffer`, живёт здесь, а не
//! разбросано по `pipeline.rs`/`sync.rs`/`context.rs`: это одна таблица,
//! загруженная в одном месте (см. doc-комментарий у `vk_load_device!` в
//! `loader.rs` — почему через `vkGetDeviceProcAddr`, а не через тот же
//! путь, что у `InstanceFns`).

use crate::vk_load_device;
use crate::vulkan::ffi::*;
use crate::vulkan::instance::Instance;
use std::ffi::c_void;

type PfnDestroyDevice = unsafe extern "system" fn(VkDevice, *const c_void);
type PfnGetDeviceQueue = unsafe extern "system" fn(VkDevice, u32, u32, *mut VkQueue);
type PfnCreateSwapchainKHR =
    unsafe extern "system" fn(VkDevice, *const VkSwapchainCreateInfoKHR, *const c_void, *mut VkSwapchainKHR) -> VkEnum;
type PfnDestroySwapchainKHR = unsafe extern "system" fn(VkDevice, VkSwapchainKHR, *const c_void);
type PfnGetSwapchainImagesKHR =
    unsafe extern "system" fn(VkDevice, VkSwapchainKHR, *mut u32, *mut VkImage) -> VkEnum;
type PfnAcquireNextImageKHR =
    unsafe extern "system" fn(VkDevice, VkSwapchainKHR, u64, VkSemaphore, VkFence, *mut u32) -> VkEnum;
type PfnQueuePresentKHR = unsafe extern "system" fn(VkQueue, *const VkPresentInfoKHR) -> VkEnum;
type PfnCreateImageView =
    unsafe extern "system" fn(VkDevice, *const VkImageViewCreateInfo, *const c_void, *mut VkImageView) -> VkEnum;
type PfnDestroyImageView = unsafe extern "system" fn(VkDevice, VkImageView, *const c_void);
type PfnCreateShaderModule =
    unsafe extern "system" fn(VkDevice, *const VkShaderModuleCreateInfo, *const c_void, *mut VkShaderModule) -> VkEnum;
type PfnDestroyShaderModule = unsafe extern "system" fn(VkDevice, VkShaderModule, *const c_void);
type PfnCreateRenderPass =
    unsafe extern "system" fn(VkDevice, *const VkRenderPassCreateInfo, *const c_void, *mut VkRenderPass) -> VkEnum;
type PfnDestroyRenderPass = unsafe extern "system" fn(VkDevice, VkRenderPass, *const c_void);
type PfnCreatePipelineLayout = unsafe extern "system" fn(
    VkDevice,
    *const VkPipelineLayoutCreateInfo,
    *const c_void,
    *mut VkPipelineLayout,
) -> VkEnum;
type PfnDestroyPipelineLayout = unsafe extern "system" fn(VkDevice, VkPipelineLayout, *const c_void);
type PfnCreateGraphicsPipelines = unsafe extern "system" fn(
    VkDevice,
    VkPipelineCache,
    u32,
    *const VkGraphicsPipelineCreateInfo,
    *const c_void,
    *mut VkPipeline,
) -> VkEnum;
type PfnDestroyPipeline = unsafe extern "system" fn(VkDevice, VkPipeline, *const c_void);
type PfnCreateFramebuffer =
    unsafe extern "system" fn(VkDevice, *const VkFramebufferCreateInfo, *const c_void, *mut VkFramebuffer) -> VkEnum;
type PfnDestroyFramebuffer = unsafe extern "system" fn(VkDevice, VkFramebuffer, *const c_void);
type PfnCreateCommandPool =
    unsafe extern "system" fn(VkDevice, *const VkCommandPoolCreateInfo, *const c_void, *mut VkCommandPool) -> VkEnum;
type PfnDestroyCommandPool = unsafe extern "system" fn(VkDevice, VkCommandPool, *const c_void);
type PfnAllocateCommandBuffers =
    unsafe extern "system" fn(VkDevice, *const VkCommandBufferAllocateInfo, *mut VkCommandBuffer) -> VkEnum;
type PfnFreeCommandBuffers = unsafe extern "system" fn(VkDevice, VkCommandPool, u32, *const VkCommandBuffer);
type PfnBeginCommandBuffer = unsafe extern "system" fn(VkCommandBuffer, *const VkCommandBufferBeginInfo) -> VkEnum;
type PfnEndCommandBuffer = unsafe extern "system" fn(VkCommandBuffer) -> VkEnum;
type PfnResetCommandBuffer = unsafe extern "system" fn(VkCommandBuffer, VkFlags) -> VkEnum;
type PfnCmdBeginRenderPass = unsafe extern "system" fn(VkCommandBuffer, *const VkRenderPassBeginInfo, VkEnum);
type PfnCmdEndRenderPass = unsafe extern "system" fn(VkCommandBuffer);
type PfnCmdBindPipeline = unsafe extern "system" fn(VkCommandBuffer, VkEnum, VkPipeline);
type PfnCmdSetViewport = unsafe extern "system" fn(VkCommandBuffer, u32, u32, *const VkViewport);
type PfnCmdSetScissor = unsafe extern "system" fn(VkCommandBuffer, u32, u32, *const VkRect2D);
type PfnCmdDraw = unsafe extern "system" fn(VkCommandBuffer, u32, u32, u32, u32);
type PfnCreateSemaphore =
    unsafe extern "system" fn(VkDevice, *const VkSemaphoreCreateInfo, *const c_void, *mut VkSemaphore) -> VkEnum;
type PfnDestroySemaphore = unsafe extern "system" fn(VkDevice, VkSemaphore, *const c_void);
type PfnCreateFence =
    unsafe extern "system" fn(VkDevice, *const VkFenceCreateInfo, *const c_void, *mut VkFence) -> VkEnum;
type PfnDestroyFence = unsafe extern "system" fn(VkDevice, VkFence, *const c_void);
type PfnWaitForFences = unsafe extern "system" fn(VkDevice, u32, *const VkFence, VkBool32, u64) -> VkEnum;
type PfnResetFences = unsafe extern "system" fn(VkDevice, u32, *const VkFence) -> VkEnum;
type PfnQueueSubmit = unsafe extern "system" fn(VkQueue, u32, *const VkSubmitInfo, VkFence) -> VkEnum;
type PfnDeviceWaitIdle = unsafe extern "system" fn(VkDevice) -> VkEnum;
type PfnCreateBuffer =
    unsafe extern "system" fn(VkDevice, *const VkBufferCreateInfo, *const c_void, *mut VkBuffer) -> VkEnum;
type PfnDestroyBuffer = unsafe extern "system" fn(VkDevice, VkBuffer, *const c_void);
type PfnGetBufferMemoryRequirements = unsafe extern "system" fn(VkDevice, VkBuffer, *mut VkMemoryRequirements);
type PfnAllocateMemory =
    unsafe extern "system" fn(VkDevice, *const VkMemoryAllocateInfo, *const c_void, *mut VkDeviceMemory) -> VkEnum;
type PfnFreeMemory = unsafe extern "system" fn(VkDevice, VkDeviceMemory, *const c_void);
type PfnBindBufferMemory = unsafe extern "system" fn(VkDevice, VkBuffer, VkDeviceMemory, VkDeviceSize) -> VkEnum;
type PfnMapMemory = unsafe extern "system" fn(
    VkDevice,
    VkDeviceMemory,
    VkDeviceSize,
    VkDeviceSize,
    VkFlags,
    *mut *mut c_void,
) -> VkEnum;
type PfnUnmapMemory = unsafe extern "system" fn(VkDevice, VkDeviceMemory);
type PfnCmdBindVertexBuffers =
    unsafe extern "system" fn(VkCommandBuffer, u32, u32, *const VkBuffer, *const VkDeviceSize);
type PfnCmdBindIndexBuffer = unsafe extern "system" fn(VkCommandBuffer, VkBuffer, VkDeviceSize, VkEnum);
type PfnCmdPushConstants =
    unsafe extern "system" fn(VkCommandBuffer, VkPipelineLayout, VkFlags, u32, u32, *const c_void);
type PfnCmdDrawIndexed = unsafe extern "system" fn(VkCommandBuffer, u32, u32, u32, i32, u32);
type PfnCreateImage =
    unsafe extern "system" fn(VkDevice, *const VkImageCreateInfo, *const c_void, *mut VkImage) -> VkEnum;
type PfnDestroyImage = unsafe extern "system" fn(VkDevice, VkImage, *const c_void);
type PfnGetImageMemoryRequirements = unsafe extern "system" fn(VkDevice, VkImage, *mut VkMemoryRequirements);
type PfnBindImageMemory = unsafe extern "system" fn(VkDevice, VkImage, VkDeviceMemory, VkDeviceSize) -> VkEnum;
type PfnCreateSampler =
    unsafe extern "system" fn(VkDevice, *const VkSamplerCreateInfo, *const c_void, *mut VkSampler) -> VkEnum;
type PfnDestroySampler = unsafe extern "system" fn(VkDevice, VkSampler, *const c_void);
type PfnCreateDescriptorSetLayout = unsafe extern "system" fn(
    VkDevice,
    *const VkDescriptorSetLayoutCreateInfo,
    *const c_void,
    *mut VkDescriptorSetLayout,
) -> VkEnum;
type PfnDestroyDescriptorSetLayout = unsafe extern "system" fn(VkDevice, VkDescriptorSetLayout, *const c_void);
type PfnCreateDescriptorPool = unsafe extern "system" fn(
    VkDevice,
    *const VkDescriptorPoolCreateInfo,
    *const c_void,
    *mut VkDescriptorPool,
) -> VkEnum;
type PfnDestroyDescriptorPool = unsafe extern "system" fn(VkDevice, VkDescriptorPool, *const c_void);
type PfnAllocateDescriptorSets =
    unsafe extern "system" fn(VkDevice, *const VkDescriptorSetAllocateInfo, *mut VkDescriptorSet) -> VkEnum;
type PfnUpdateDescriptorSets =
    unsafe extern "system" fn(VkDevice, u32, *const VkWriteDescriptorSet, u32, *const c_void);
type PfnCmdPipelineBarrier = unsafe extern "system" fn(
    VkCommandBuffer,
    VkFlags,
    VkFlags,
    VkFlags,
    u32,
    *const c_void,
    u32,
    *const c_void,
    u32,
    *const VkImageMemoryBarrier,
);
type PfnCmdCopyBufferToImage =
    unsafe extern "system" fn(VkCommandBuffer, VkBuffer, VkImage, VkEnum, u32, *const VkBufferImageCopy);
type PfnCmdBindDescriptorSets = unsafe extern "system" fn(
    VkCommandBuffer,
    VkEnum,
    VkPipelineLayout,
    u32,
    u32,
    *const VkDescriptorSet,
    u32,
    *const u32,
);
type PfnQueueWaitIdle = unsafe extern "system" fn(VkQueue) -> VkEnum;

/// Все функции, которые дальше принимают `VkDevice`/`VkQueue`/
/// `VkCommandBuffer` — от создания swapchain до записи команд в буфер.
/// Поля названы точно как функции в спецификации (без префикса `vk`), чтобы
/// вызов `device.fns.cmd_draw(...)` читался как обычный Vulkan-код
#[allow(dead_code)] // часть функций понадобится в Фазах 2+, не в Фазе 1
pub struct DeviceFns {
    pub destroy_device: PfnDestroyDevice,
    pub get_device_queue: PfnGetDeviceQueue,
    pub create_swapchain_khr: PfnCreateSwapchainKHR,
    pub destroy_swapchain_khr: PfnDestroySwapchainKHR,
    pub get_swapchain_images_khr: PfnGetSwapchainImagesKHR,
    pub acquire_next_image_khr: PfnAcquireNextImageKHR,
    pub queue_present_khr: PfnQueuePresentKHR,
    pub create_image_view: PfnCreateImageView,
    pub destroy_image_view: PfnDestroyImageView,
    pub create_shader_module: PfnCreateShaderModule,
    pub destroy_shader_module: PfnDestroyShaderModule,
    pub create_render_pass: PfnCreateRenderPass,
    pub destroy_render_pass: PfnDestroyRenderPass,
    pub create_pipeline_layout: PfnCreatePipelineLayout,
    pub destroy_pipeline_layout: PfnDestroyPipelineLayout,
    pub create_graphics_pipelines: PfnCreateGraphicsPipelines,
    pub destroy_pipeline: PfnDestroyPipeline,
    pub create_framebuffer: PfnCreateFramebuffer,
    pub destroy_framebuffer: PfnDestroyFramebuffer,
    pub create_command_pool: PfnCreateCommandPool,
    pub destroy_command_pool: PfnDestroyCommandPool,
    pub allocate_command_buffers: PfnAllocateCommandBuffers,
    pub free_command_buffers: PfnFreeCommandBuffers,
    pub begin_command_buffer: PfnBeginCommandBuffer,
    pub end_command_buffer: PfnEndCommandBuffer,
    pub reset_command_buffer: PfnResetCommandBuffer,
    pub cmd_begin_render_pass: PfnCmdBeginRenderPass,
    pub cmd_end_render_pass: PfnCmdEndRenderPass,
    pub cmd_bind_pipeline: PfnCmdBindPipeline,
    pub cmd_set_viewport: PfnCmdSetViewport,
    pub cmd_set_scissor: PfnCmdSetScissor,
    pub cmd_draw: PfnCmdDraw,
    pub create_semaphore: PfnCreateSemaphore,
    pub destroy_semaphore: PfnDestroySemaphore,
    pub create_fence: PfnCreateFence,
    pub destroy_fence: PfnDestroyFence,
    pub wait_for_fences: PfnWaitForFences,
    pub reset_fences: PfnResetFences,
    pub queue_submit: PfnQueueSubmit,
    pub device_wait_idle: PfnDeviceWaitIdle,
    pub create_buffer: PfnCreateBuffer,
    pub destroy_buffer: PfnDestroyBuffer,
    pub get_buffer_memory_requirements: PfnGetBufferMemoryRequirements,
    pub allocate_memory: PfnAllocateMemory,
    pub free_memory: PfnFreeMemory,
    pub bind_buffer_memory: PfnBindBufferMemory,
    pub map_memory: PfnMapMemory,
    pub unmap_memory: PfnUnmapMemory,
    pub cmd_bind_vertex_buffers: PfnCmdBindVertexBuffers,
    pub cmd_bind_index_buffer: PfnCmdBindIndexBuffer,
    pub cmd_push_constants: PfnCmdPushConstants,
    pub cmd_draw_indexed: PfnCmdDrawIndexed,
    pub create_image: PfnCreateImage,
    pub destroy_image: PfnDestroyImage,
    pub get_image_memory_requirements: PfnGetImageMemoryRequirements,
    pub bind_image_memory: PfnBindImageMemory,
    pub create_sampler: PfnCreateSampler,
    pub destroy_sampler: PfnDestroySampler,
    pub create_descriptor_set_layout: PfnCreateDescriptorSetLayout,
    pub destroy_descriptor_set_layout: PfnDestroyDescriptorSetLayout,
    pub create_descriptor_pool: PfnCreateDescriptorPool,
    pub destroy_descriptor_pool: PfnDestroyDescriptorPool,
    pub allocate_descriptor_sets: PfnAllocateDescriptorSets,
    pub update_descriptor_sets: PfnUpdateDescriptorSets,
    pub cmd_pipeline_barrier: PfnCmdPipelineBarrier,
    pub cmd_copy_buffer_to_image: PfnCmdCopyBufferToImage,
    pub cmd_bind_descriptor_sets: PfnCmdBindDescriptorSets,
    pub queue_wait_idle: PfnQueueWaitIdle,
}

pub struct Device {
    pub handle: VkDevice,
    pub physical: VkPhysicalDevice,
    pub queue: VkQueue,
    pub queue_family: u32,
    pub fns: DeviceFns,
}

impl Device {
    /// Берёт первое физическое устройство, у которого есть очередь и с
    /// графикой, и с поддержкой презентации в `surface` — для «hello
    /// triangle» разбираться дальше (интегрированная/дискретная,
    /// сколько памяти) незачем: любое такое устройство нарисует треугольник
    pub fn new(instance: &Instance, surface: VkSurfaceKHR) -> Result<Self, String> {
        let (physical, queue_family) = pick_physical_device(instance, surface)?;

        let priority: f32 = 1.0;
        let queue_create_info = VkDeviceQueueCreateInfo {
            s_type: VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,
            p_next: std::ptr::null(),
            flags: 0,
            queue_family_index: queue_family,
            queue_count: 1,
            p_queue_priorities: &priority,
        };

        let swapchain_ext = c"VK_KHR_swapchain";
        let extensions = [swapchain_ext.as_ptr()];

        let create_info = VkDeviceCreateInfo {
            s_type: VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO,
            p_next: std::ptr::null(),
            flags: 0,
            queue_create_info_count: 1,
            p_queue_create_infos: &queue_create_info,
            enabled_layer_count: 0,
            pp_enabled_layer_names: std::ptr::null(),
            enabled_extension_count: extensions.len() as u32,
            pp_enabled_extension_names: extensions.as_ptr(),
            p_enabled_features: std::ptr::null(),
        };

        let mut handle = VkDevice::NULL;
        let result = unsafe { (instance.fns.create_device)(physical, &create_info, std::ptr::null(), &mut handle) };
        if result != VK_SUCCESS {
            return Err(format!("vkCreateDevice вернул {result}"));
        }

        let get_device_proc_addr = instance.fns.get_device_proc_addr;
        let fns = DeviceFns {
            destroy_device: vk_load_device!(get_device_proc_addr, handle, "vkDestroyDevice", PfnDestroyDevice),
            get_device_queue: vk_load_device!(get_device_proc_addr, handle, "vkGetDeviceQueue", PfnGetDeviceQueue),
            create_swapchain_khr: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkCreateSwapchainKHR",
                PfnCreateSwapchainKHR
            ),
            destroy_swapchain_khr: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkDestroySwapchainKHR",
                PfnDestroySwapchainKHR
            ),
            get_swapchain_images_khr: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkGetSwapchainImagesKHR",
                PfnGetSwapchainImagesKHR
            ),
            acquire_next_image_khr: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkAcquireNextImageKHR",
                PfnAcquireNextImageKHR
            ),
            queue_present_khr: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkQueuePresentKHR",
                PfnQueuePresentKHR
            ),
            create_image_view: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkCreateImageView",
                PfnCreateImageView
            ),
            destroy_image_view: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkDestroyImageView",
                PfnDestroyImageView
            ),
            create_shader_module: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkCreateShaderModule",
                PfnCreateShaderModule
            ),
            destroy_shader_module: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkDestroyShaderModule",
                PfnDestroyShaderModule
            ),
            create_render_pass: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkCreateRenderPass",
                PfnCreateRenderPass
            ),
            destroy_render_pass: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkDestroyRenderPass",
                PfnDestroyRenderPass
            ),
            create_pipeline_layout: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkCreatePipelineLayout",
                PfnCreatePipelineLayout
            ),
            destroy_pipeline_layout: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkDestroyPipelineLayout",
                PfnDestroyPipelineLayout
            ),
            create_graphics_pipelines: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkCreateGraphicsPipelines",
                PfnCreateGraphicsPipelines
            ),
            destroy_pipeline: vk_load_device!(get_device_proc_addr, handle, "vkDestroyPipeline", PfnDestroyPipeline),
            create_framebuffer: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkCreateFramebuffer",
                PfnCreateFramebuffer
            ),
            destroy_framebuffer: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkDestroyFramebuffer",
                PfnDestroyFramebuffer
            ),
            create_command_pool: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkCreateCommandPool",
                PfnCreateCommandPool
            ),
            destroy_command_pool: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkDestroyCommandPool",
                PfnDestroyCommandPool
            ),
            allocate_command_buffers: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkAllocateCommandBuffers",
                PfnAllocateCommandBuffers
            ),
            free_command_buffers: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkFreeCommandBuffers",
                PfnFreeCommandBuffers
            ),
            begin_command_buffer: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkBeginCommandBuffer",
                PfnBeginCommandBuffer
            ),
            end_command_buffer: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkEndCommandBuffer",
                PfnEndCommandBuffer
            ),
            reset_command_buffer: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkResetCommandBuffer",
                PfnResetCommandBuffer
            ),
            cmd_begin_render_pass: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkCmdBeginRenderPass",
                PfnCmdBeginRenderPass
            ),
            cmd_end_render_pass: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkCmdEndRenderPass",
                PfnCmdEndRenderPass
            ),
            cmd_bind_pipeline: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkCmdBindPipeline",
                PfnCmdBindPipeline
            ),
            cmd_set_viewport: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkCmdSetViewport",
                PfnCmdSetViewport
            ),
            cmd_set_scissor: vk_load_device!(get_device_proc_addr, handle, "vkCmdSetScissor", PfnCmdSetScissor),
            cmd_draw: vk_load_device!(get_device_proc_addr, handle, "vkCmdDraw", PfnCmdDraw),
            create_semaphore: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkCreateSemaphore",
                PfnCreateSemaphore
            ),
            destroy_semaphore: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkDestroySemaphore",
                PfnDestroySemaphore
            ),
            create_fence: vk_load_device!(get_device_proc_addr, handle, "vkCreateFence", PfnCreateFence),
            destroy_fence: vk_load_device!(get_device_proc_addr, handle, "vkDestroyFence", PfnDestroyFence),
            wait_for_fences: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkWaitForFences",
                PfnWaitForFences
            ),
            reset_fences: vk_load_device!(get_device_proc_addr, handle, "vkResetFences", PfnResetFences),
            queue_submit: vk_load_device!(get_device_proc_addr, handle, "vkQueueSubmit", PfnQueueSubmit),
            device_wait_idle: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkDeviceWaitIdle",
                PfnDeviceWaitIdle
            ),
            create_buffer: vk_load_device!(get_device_proc_addr, handle, "vkCreateBuffer", PfnCreateBuffer),
            destroy_buffer: vk_load_device!(get_device_proc_addr, handle, "vkDestroyBuffer", PfnDestroyBuffer),
            get_buffer_memory_requirements: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkGetBufferMemoryRequirements",
                PfnGetBufferMemoryRequirements
            ),
            allocate_memory: vk_load_device!(get_device_proc_addr, handle, "vkAllocateMemory", PfnAllocateMemory),
            free_memory: vk_load_device!(get_device_proc_addr, handle, "vkFreeMemory", PfnFreeMemory),
            bind_buffer_memory: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkBindBufferMemory",
                PfnBindBufferMemory
            ),
            map_memory: vk_load_device!(get_device_proc_addr, handle, "vkMapMemory", PfnMapMemory),
            unmap_memory: vk_load_device!(get_device_proc_addr, handle, "vkUnmapMemory", PfnUnmapMemory),
            cmd_bind_vertex_buffers: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkCmdBindVertexBuffers",
                PfnCmdBindVertexBuffers
            ),
            cmd_bind_index_buffer: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkCmdBindIndexBuffer",
                PfnCmdBindIndexBuffer
            ),
            cmd_push_constants: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkCmdPushConstants",
                PfnCmdPushConstants
            ),
            cmd_draw_indexed: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkCmdDrawIndexed",
                PfnCmdDrawIndexed
            ),
            create_image: vk_load_device!(get_device_proc_addr, handle, "vkCreateImage", PfnCreateImage),
            destroy_image: vk_load_device!(get_device_proc_addr, handle, "vkDestroyImage", PfnDestroyImage),
            get_image_memory_requirements: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkGetImageMemoryRequirements",
                PfnGetImageMemoryRequirements
            ),
            bind_image_memory: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkBindImageMemory",
                PfnBindImageMemory
            ),
            create_sampler: vk_load_device!(get_device_proc_addr, handle, "vkCreateSampler", PfnCreateSampler),
            destroy_sampler: vk_load_device!(get_device_proc_addr, handle, "vkDestroySampler", PfnDestroySampler),
            create_descriptor_set_layout: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkCreateDescriptorSetLayout",
                PfnCreateDescriptorSetLayout
            ),
            destroy_descriptor_set_layout: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkDestroyDescriptorSetLayout",
                PfnDestroyDescriptorSetLayout
            ),
            create_descriptor_pool: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkCreateDescriptorPool",
                PfnCreateDescriptorPool
            ),
            destroy_descriptor_pool: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkDestroyDescriptorPool",
                PfnDestroyDescriptorPool
            ),
            allocate_descriptor_sets: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkAllocateDescriptorSets",
                PfnAllocateDescriptorSets
            ),
            update_descriptor_sets: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkUpdateDescriptorSets",
                PfnUpdateDescriptorSets
            ),
            cmd_pipeline_barrier: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkCmdPipelineBarrier",
                PfnCmdPipelineBarrier
            ),
            cmd_copy_buffer_to_image: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkCmdCopyBufferToImage",
                PfnCmdCopyBufferToImage
            ),
            cmd_bind_descriptor_sets: vk_load_device!(
                get_device_proc_addr,
                handle,
                "vkCmdBindDescriptorSets",
                PfnCmdBindDescriptorSets
            ),
            queue_wait_idle: vk_load_device!(get_device_proc_addr, handle, "vkQueueWaitIdle", PfnQueueWaitIdle),
        };

        let mut queue = VkQueue::NULL;
        unsafe {
            (fns.get_device_queue)(handle, queue_family, 0, &mut queue);
        }

        Ok(Self { handle, physical, queue, queue_family, fns })
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        // Логическое устройство закрывается раньше instance (об этом
        // заботится порядок полей в `Renderer`, см. `context.rs`), но сам
        // GPU может ещё выполнять посланные ему команды — без
        // `vkDeviceWaitIdle` уничтожение соскочит из-под работающего
        // конвейера, и последний кадр развалится посреди отрисовки
        unsafe {
            (self.fns.device_wait_idle)(self.handle);
            (self.fns.destroy_device)(self.handle, std::ptr::null());
        }
    }
}

/// Перебирает физические устройства и берёт первое, у которого нашлось
/// семейство очередей и с `VK_QUEUE_GRAPHICS_BIT`, и с поддержкой
/// презентации в переданную поверхность. Обе проверки обязаны пройти на
/// ОДНОМ И ТОМ ЖЕ индексе семейства не всегда — спецификация this не
/// гарантирует, но в подавляющем большинстве реального железа это одно и
/// то же семейство, и заводить отдельную очередь под презентацию в Фазе 1
/// избыточно
fn pick_physical_device(instance: &Instance, surface: VkSurfaceKHR) -> Result<(VkPhysicalDevice, u32), String> {
    let mut count = 0u32;
    unsafe {
        (instance.fns.enumerate_physical_devices)(instance.handle, &mut count, std::ptr::null_mut());
    }
    if count == 0 {
        return Err("на этой машине Vulkan не видит ни одного физического устройства".into());
    }

    let mut devices = vec![VkPhysicalDevice::NULL; count as usize];
    unsafe {
        (instance.fns.enumerate_physical_devices)(instance.handle, &mut count, devices.as_mut_ptr());
    }

    for physical in devices {
        let mut family_count = 0u32;
        unsafe {
            (instance.fns.get_physical_device_queue_family_properties)(
                physical,
                &mut family_count,
                std::ptr::null_mut(),
            );
        }
        let mut families = vec![VkQueueFamilyProperties::default(); family_count as usize];
        unsafe {
            (instance.fns.get_physical_device_queue_family_properties)(
                physical,
                &mut family_count,
                families.as_mut_ptr(),
            );
        }

        for (index, family) in families.iter().enumerate() {
            let index = index as u32;
            let graphics = family.queue_flags & VK_QUEUE_GRAPHICS_BIT != 0;
            if !graphics {
                continue;
            }

            let mut present_supported: VkBool32 = VK_FALSE;
            unsafe {
                (instance.fns.get_physical_device_surface_support_khr)(
                    physical,
                    index,
                    surface,
                    &mut present_supported,
                );
            }
            if present_supported == VK_TRUE {
                return Ok((physical, index));
            }
        }
    }

    Err("ни одно физическое устройство не поддерживает и графику, и презентацию в это окно".into())
}
