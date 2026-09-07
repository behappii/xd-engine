//! Swapchain — кольцо картинок, которые по очереди показываются в окне, и
//! их image view (без view картинку нельзя привязать к render pass —
//! Vulkan везде смотрит на ресурсы через view, а не напрямую на `VkImage`).
//!
//! Выбор формата/режима показа/размера вынесен в чистые функции без единого
//! вызова Vulkan — по тому же принципу, что `frame_size`/`adopt_frame_size`
//! в `app.rs`: их логика уже один раз ловила баги на границах (свёрнутое
//! окно, зажим размера), и здесь у неё прямые аналоги

use crate::vulkan::device::Device;
use crate::vulkan::ffi::*;
use crate::vulkan::instance::Instance;

pub struct Swapchain {
    pub handle: VkSwapchainKHR,
    pub images: Vec<VkImage>,
    pub image_views: Vec<VkImageView>,
    pub format: VkEnum,
    pub extent: VkExtent2D,
}

impl Swapchain {
    pub fn new(
        instance: &Instance,
        device: &Device,
        surface: VkSurfaceKHR,
        window_size: (u32, u32),
    ) -> Result<Self, String> {
        let capabilities = query_capabilities(instance, device, surface);
        let formats = query_formats(instance, device, surface);
        let present_modes = query_present_modes(instance, device, surface);

        let surface_format = choose_surface_format(&formats);
        let present_mode = choose_present_mode(&present_modes);
        let extent = choose_extent(&capabilities, window_size.0, window_size.1);
        let image_count = choose_image_count(&capabilities);

        let create_info = VkSwapchainCreateInfoKHR {
            s_type: VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR,
            p_next: std::ptr::null(),
            flags: 0,
            surface,
            min_image_count: image_count,
            image_format: surface_format.format,
            image_color_space: surface_format.color_space,
            image_extent: extent,
            image_array_layers: 1,
            image_usage: VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT,
            // EXCLUSIVE: у нас одна очередь и на графику, и на презентацию
            // (см. `pick_physical_device` в `device.rs`) — делить владение
            // картинкой между очередями незачем
            image_sharing_mode: VK_SHARING_MODE_EXCLUSIVE,
            queue_family_index_count: 0,
            p_queue_family_indices: std::ptr::null(),
            pre_transform: capabilities.current_transform,
            composite_alpha: VK_COMPOSITE_ALPHA_OPAQUE_BIT_KHR,
            present_mode,
            clipped: VK_TRUE,
            old_swapchain: VkSwapchainKHR::NULL,
        };

        let mut handle = VkSwapchainKHR::NULL;
        let result =
            unsafe { (device.fns.create_swapchain_khr)(device.handle, &create_info, std::ptr::null(), &mut handle) };
        if result != VK_SUCCESS {
            return Err(format!("vkCreateSwapchainKHR вернул {result}"));
        }

        let images = fetch_swapchain_images(device, handle);
        let image_views = create_image_views(device, &images, surface_format.format)?;

        Ok(Self { handle, images, image_views, format: surface_format.format, extent })
    }

    /// Не `Drop`: уничтожение требует device-таблицу функций, которой у
    /// `Swapchain` своей нет (см. объяснение в `context.rs` — почему
    /// порядок закрытия объектов там прописан явно, а не выведен из
    /// порядка полей структуры)
    pub fn destroy(&mut self, device: &Device) {
        for &view in &self.image_views {
            unsafe {
                (device.fns.destroy_image_view)(device.handle, view, std::ptr::null());
            }
        }
        self.image_views.clear();
        unsafe {
            (device.fns.destroy_swapchain_khr)(device.handle, self.handle, std::ptr::null());
        }
        self.handle = VkSwapchainKHR::NULL;
    }
}

fn query_capabilities(instance: &Instance, device: &Device, surface: VkSurfaceKHR) -> VkSurfaceCapabilitiesKHR {
    let mut capabilities = VkSurfaceCapabilitiesKHR::default();
    unsafe {
        (instance.fns.get_physical_device_surface_capabilities_khr)(device.physical, surface, &mut capabilities);
    }
    capabilities
}

fn query_formats(instance: &Instance, device: &Device, surface: VkSurfaceKHR) -> Vec<VkSurfaceFormatKHR> {
    let mut count = 0u32;
    unsafe {
        (instance.fns.get_physical_device_surface_formats_khr)(device.physical, surface, &mut count, std::ptr::null_mut());
    }
    let mut formats = vec![VkSurfaceFormatKHR { format: VK_FORMAT_UNDEFINED, color_space: 0 }; count as usize];
    unsafe {
        (instance.fns.get_physical_device_surface_formats_khr)(device.physical, surface, &mut count, formats.as_mut_ptr());
    }
    formats
}

fn query_present_modes(instance: &Instance, device: &Device, surface: VkSurfaceKHR) -> Vec<VkEnum> {
    let mut count = 0u32;
    unsafe {
        (instance.fns.get_physical_device_surface_present_modes_khr)(
            device.physical,
            surface,
            &mut count,
            std::ptr::null_mut(),
        );
    }
    let mut modes = vec![0 as VkEnum; count as usize];
    unsafe {
        (instance.fns.get_physical_device_surface_present_modes_khr)(
            device.physical,
            surface,
            &mut count,
            modes.as_mut_ptr(),
        );
    }
    modes
}

fn fetch_swapchain_images(device: &Device, swapchain: VkSwapchainKHR) -> Vec<VkImage> {
    let mut count = 0u32;
    unsafe {
        (device.fns.get_swapchain_images_khr)(device.handle, swapchain, &mut count, std::ptr::null_mut());
    }
    let mut images = vec![VkImage::NULL; count as usize];
    unsafe {
        (device.fns.get_swapchain_images_khr)(device.handle, swapchain, &mut count, images.as_mut_ptr());
    }
    images
}

fn create_image_views(device: &Device, images: &[VkImage], format: VkEnum) -> Result<Vec<VkImageView>, String> {
    let mut views = Vec::with_capacity(images.len());
    for &image in images {
        let identity = VkComponentMapping {
            r: VK_COMPONENT_SWIZZLE_IDENTITY,
            g: VK_COMPONENT_SWIZZLE_IDENTITY,
            b: VK_COMPONENT_SWIZZLE_IDENTITY,
            a: VK_COMPONENT_SWIZZLE_IDENTITY,
        };
        let subresource_range = VkImageSubresourceRange {
            aspect_mask: VK_IMAGE_ASPECT_COLOR_BIT,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        };
        let view_info = VkImageViewCreateInfo {
            s_type: VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO,
            p_next: std::ptr::null(),
            flags: 0,
            image,
            view_type: VK_IMAGE_VIEW_TYPE_2D,
            format,
            components: identity,
            subresource_range,
        };
        let mut view = VkImageView::NULL;
        let result = unsafe { (device.fns.create_image_view)(device.handle, &view_info, std::ptr::null(), &mut view) };
        if result != VK_SUCCESS {
            return Err(format!("vkCreateImageView вернул {result}"));
        }
        views.push(view);
    }
    Ok(views)
}

// ============================================================================
// Чистые функции выбора — без единого вызова Vulkan, поэтому проверяются
// юнит-тестами без окна и без GPU
// ============================================================================

/// Предпочитаем `B8G8R8A8_UNORM` + `SRGB_NONLINEAR` — самая ходовая пара
/// форматов, поддержана практически везде, — но если её нет, берём первый
/// доступный: список гарантированно непустой для валидной поверхности
/// (спецификация Vulkan это обещает), а откатиться на «то, что реально
/// умеет драйвер» лучше, чем упасть из-за одной неверной догадки
fn choose_surface_format(available: &[VkSurfaceFormatKHR]) -> VkSurfaceFormatKHR {
    available
        .iter()
        .find(|f| f.format == VK_FORMAT_B8G8R8A8_UNORM && f.color_space == VK_COLOR_SPACE_SRGB_NONLINEAR_KHR)
        .copied()
        .unwrap_or(available[0])
}

/// `FIFO` — единственный режим показа, который спецификация Vulkan
/// гарантирует доступным всегда (аналог вертикальной синхронизации). Более
/// быстрые режимы (`MAILBOX`, без ожидания вертикальной развёртки) —
/// оптимизация будущей фазы, не Фазы 1: здесь важнее гарантированно
/// заработать, чем минимальная задержка кадра
fn choose_present_mode(_available: &[VkEnum]) -> VkEnum {
    VK_PRESENT_MODE_FIFO_KHR
}

/// `currentExtent.width == u32::MAX` — сигнал от Vulkan «размер поверхности
/// назначаешь ты сам», иначе поверхность жёстко диктует размер, и он
/// обязан совпасть с тем, что попросили при создании swapchain
fn choose_extent(capabilities: &VkSurfaceCapabilitiesKHR, window_width: u32, window_height: u32) -> VkExtent2D {
    if capabilities.current_extent.width != u32::MAX {
        return capabilities.current_extent;
    }
    VkExtent2D {
        width: window_width.clamp(capabilities.min_image_extent.width, capabilities.max_image_extent.width),
        height: window_height.clamp(capabilities.min_image_extent.height, capabilities.max_image_extent.height),
    }
}

/// `minImageCount + 1`: с ровно минимумом драйвер иногда заставляет ждать
/// освобождения кадра перед тем, как начать рисовать следующий — на одну
/// картинку больше почти всегда даёт этому запас. `maxImageCount == 0`
/// означает «драйвер не ограничивает», а не «ноль картинок»
fn choose_image_count(capabilities: &VkSurfaceCapabilitiesKHR) -> u32 {
    let preferred = capabilities.min_image_count + 1;
    if capabilities.max_image_count > 0 { preferred.min(capabilities.max_image_count) } else { preferred }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_bgra8_srgb_nonlinear_when_available() {
        let formats = [
            VkSurfaceFormatKHR { format: VK_FORMAT_UNDEFINED, color_space: 5 },
            VkSurfaceFormatKHR { format: VK_FORMAT_B8G8R8A8_UNORM, color_space: VK_COLOR_SPACE_SRGB_NONLINEAR_KHR },
        ];
        let chosen = choose_surface_format(&formats);
        assert_eq!(chosen.format, VK_FORMAT_B8G8R8A8_UNORM);
        assert_eq!(chosen.color_space, VK_COLOR_SPACE_SRGB_NONLINEAR_KHR);
    }

    #[test]
    fn falls_back_to_the_first_format_when_the_preferred_pair_is_missing() {
        // Пара не встречается целиком (формат есть, цветовое пространство —
        // нет), значит откатываемся на первый в списке, а не подбираем её
        // по частям
        let formats = [VkSurfaceFormatKHR { format: 123, color_space: 7 }];
        let chosen = choose_surface_format(&formats);
        assert_eq!((chosen.format, chosen.color_space), (123, 7));
    }

    #[test]
    fn present_mode_is_always_fifo_regardless_of_what_is_offered() {
        assert_eq!(choose_present_mode(&[]), VK_PRESENT_MODE_FIFO_KHR);
        assert_eq!(choose_present_mode(&[0, 1, 2, 3]), VK_PRESENT_MODE_FIFO_KHR);
    }

    #[test]
    fn extent_follows_the_surface_when_it_dictates_a_fixed_size() {
        let caps = VkSurfaceCapabilitiesKHR {
            current_extent: VkExtent2D { width: 800, height: 600 },
            ..Default::default()
        };
        let extent = choose_extent(&caps, 1920, 1080);
        assert_eq!((extent.width, extent.height), (800, 600));
    }

    #[test]
    fn extent_falls_back_to_the_window_size_clamped_to_the_range() {
        let caps = VkSurfaceCapabilitiesKHR {
            current_extent: VkExtent2D { width: u32::MAX, height: u32::MAX },
            min_image_extent: VkExtent2D { width: 100, height: 100 },
            max_image_extent: VkExtent2D { width: 4096, height: 4096 },
            ..Default::default()
        };
        // По ширине окно МЕНЬШЕ разрешённого минимума, по высоте — БОЛЬШЕ
        // разрешённого максимума: зажим обязан сработать в обе стороны
        let extent = choose_extent(&caps, 50, 8000);
        assert_eq!((extent.width, extent.height), (100, 4096));
    }

    #[test]
    fn image_count_asks_for_one_more_than_minimum() {
        let caps = VkSurfaceCapabilitiesKHR { min_image_count: 2, max_image_count: 8, ..Default::default() };
        assert_eq!(choose_image_count(&caps), 3);
    }

    #[test]
    fn image_count_is_clamped_to_the_maximum() {
        let caps = VkSurfaceCapabilitiesKHR { min_image_count: 3, max_image_count: 3, ..Default::default() };
        assert_eq!(choose_image_count(&caps), 3);
    }

    #[test]
    fn image_count_has_no_ceiling_when_the_driver_does_not_set_one() {
        // maxImageCount == 0 по спецификации Vulkan значит «без
        // ограничения», а не «ноль картинок» — перепутать легко
        let caps = VkSurfaceCapabilitiesKHR { min_image_count: 3, max_image_count: 0, ..Default::default() };
        assert_eq!(choose_image_count(&caps), 4);
    }
}
