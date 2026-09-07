//! Depth-буфер: `VkImage`+`VkImageView`, которые GPU сам заполняет во время
//! рендеринга (тест глубины пишет и читает его прямо в render pass) — в
//! отличие от `image.rs`, сюда никогда не грузятся байты с CPU, поэтому нет
//! ни staging-буфера, ни собственного барьера перехода раскладки. Картинка
//! создаётся, как и текстурная, в `VK_IMAGE_LAYOUT_UNDEFINED`, а в рабочую
//! `DEPTH_STENCIL_ATTACHMENT_OPTIMAL` её переводит САМ render pass при входе
//! в подпасс — у него на этот attachment объявлены ровно та же пара
//! `initial_layout`/`final_layout` (см. `pipeline::create_render_pass`).
//! Отдельному `vkCmdPipelineBarrier`, как в `image.rs`, здесь просто нечего
//! делать: там переход нужен между двумя командами ВНЕ render pass'а.
//!
//! **Почему формат `D32_SFLOAT`.** Безусловно, на любой реализации,
//! спецификация гарантирует как depth attachment только `D16_UNORM`; про
//! `D32_SFLOAT` она обещает слабее — что поддержан ХОТЯ БЫ ОДИН из
//! `X8_D24_UNORM_PACK32` и `D32_SFLOAT`. На практике `D32_SFLOAT` есть
//! везде, где вообще есть Vulkan, поэтому он и зашит константой, но это
//! допущение, а не гарантия: честный путь — спросить у устройства
//! `vkGetPhysicalDeviceFormatProperties` и выбрать первый подходящий формат
//! из списка предпочтений. Пока не сделано (записано в пробелы).
//!
//! **Один буфер на весь свопчейн, не по одному на картинку.** У цветных
//! attachment'ов своя картинка на кадр в полёте — сколько кадров может
//! рисоваться параллельно, столько и картинок. Глубина же нужна ровно на
//! время ОДНОГО кадра (очищается заново каждый раз) и не идёт на экран, а
//! рендерер держит только один кадр в полёте (см. doc-комментарий
//! `context.rs`) — значит одного буфера достаточно, и делить его между
//! framebuffer'ами разных swapchain-картинок безопасно.

use crate::vulkan::buffer;
use crate::vulkan::device::Device;
use crate::vulkan::ffi::*;

pub struct DepthBuffer {
    pub view: VkImageView,
    image: VkImage,
    memory: VkDeviceMemory,
}

impl DepthBuffer {
    pub fn new(
        device: &Device,
        memory_properties: &VkPhysicalDeviceMemoryProperties,
        extent: VkExtent2D,
    ) -> Result<Self, String> {
        let create_info = VkImageCreateInfo {
            s_type: VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO,
            p_next: std::ptr::null(),
            flags: 0,
            image_type: VK_IMAGE_TYPE_2D,
            format: VK_FORMAT_D32_SFLOAT,
            extent: VkExtent3D { width: extent.width, height: extent.height, depth: 1 },
            mip_levels: 1,
            array_layers: 1,
            samples: VK_SAMPLE_COUNT_1_BIT,
            tiling: VK_IMAGE_TILING_OPTIMAL,
            usage: VK_IMAGE_USAGE_DEPTH_STENCIL_ATTACHMENT_BIT,
            sharing_mode: VK_SHARING_MODE_EXCLUSIVE,
            queue_family_index_count: 0,
            p_queue_family_indices: std::ptr::null(),
            initial_layout: VK_IMAGE_LAYOUT_UNDEFINED,
        };
        let mut image = VkImage::NULL;
        let result = unsafe { (device.fns.create_image)(device.handle, &create_info, std::ptr::null(), &mut image) };
        if result != VK_SUCCESS {
            return Err(format!("vkCreateImage (глубина) вернул {result}"));
        }

        let mut requirements = VkMemoryRequirements::default();
        unsafe {
            (device.fns.get_image_memory_requirements)(device.handle, image, &mut requirements);
        }

        let memory_type_index = match buffer::find_memory_type(
            memory_properties,
            requirements.memory_type_bits,
            VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT,
        ) {
            Ok(index) => index,
            Err(err) => {
                unsafe {
                    (device.fns.destroy_image)(device.handle, image, std::ptr::null());
                }
                return Err(err);
            }
        };

        let allocate_info = VkMemoryAllocateInfo {
            s_type: VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
            p_next: std::ptr::null(),
            allocation_size: requirements.size,
            memory_type_index,
        };
        let mut memory = VkDeviceMemory::NULL;
        let result =
            unsafe { (device.fns.allocate_memory)(device.handle, &allocate_info, std::ptr::null(), &mut memory) };
        if result != VK_SUCCESS {
            unsafe {
                (device.fns.destroy_image)(device.handle, image, std::ptr::null());
            }
            return Err(format!("vkAllocateMemory (глубина) вернул {result}"));
        }

        let result = unsafe { (device.fns.bind_image_memory)(device.handle, image, memory, 0) };
        if result != VK_SUCCESS {
            unsafe {
                (device.fns.free_memory)(device.handle, memory, std::ptr::null());
                (device.fns.destroy_image)(device.handle, image, std::ptr::null());
            }
            return Err(format!("vkBindImageMemory (глубина) вернул {result}"));
        }

        let view_info = VkImageViewCreateInfo {
            s_type: VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO,
            p_next: std::ptr::null(),
            flags: 0,
            image,
            view_type: VK_IMAGE_VIEW_TYPE_2D,
            format: VK_FORMAT_D32_SFLOAT,
            components: VkComponentMapping {
                r: VK_COMPONENT_SWIZZLE_IDENTITY,
                g: VK_COMPONENT_SWIZZLE_IDENTITY,
                b: VK_COMPONENT_SWIZZLE_IDENTITY,
                a: VK_COMPONENT_SWIZZLE_IDENTITY,
            },
            subresource_range: VkImageSubresourceRange {
                aspect_mask: VK_IMAGE_ASPECT_DEPTH_BIT,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            },
        };
        let mut view = VkImageView::NULL;
        let result =
            unsafe { (device.fns.create_image_view)(device.handle, &view_info, std::ptr::null(), &mut view) };
        if result != VK_SUCCESS {
            unsafe {
                (device.fns.free_memory)(device.handle, memory, std::ptr::null());
                (device.fns.destroy_image)(device.handle, image, std::ptr::null());
            }
            return Err(format!("vkCreateImageView (глубина) вернул {result}"));
        }

        Ok(Self { view, image, memory })
    }

    pub fn destroy(&self, device: &Device) {
        unsafe {
            (device.fns.destroy_image_view)(device.handle, self.view, std::ptr::null());
            (device.fns.destroy_image)(device.handle, self.image, std::ptr::null());
            (device.fns.free_memory)(device.handle, self.memory, std::ptr::null());
        }
    }
}
