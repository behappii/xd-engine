//! Изображение на GPU: `VkImage` + его память + `VkImageView` — то, что
//! в итоге видит сэмплер в шейдере. Загружает сюда сырые тексели CPU-текстуры
//! (`crate::texture::Texture::level0_rgba8`) — Фаза 4.
//!
//! **Почему не host-visible, как вершинный/индексный буфер.** У буферов
//! (Фаза 2) host-visible память была осознанным упрощением, задокументированным
//! как таковое там же. Для картинки так рисковать нельзя: `OPTIMAL`-тайлинг
//! (единственный, который реально быстро сэмплится на настоящем железе —
//! это его собственная, недокументированная наружу раскладка байт) в
//! принципе НЕ БЫВАЕТ `HOST_VISIBLE`, а `LINEAR`-тайлинг спецификация не
//! обязана поддерживать для выборки в шейдере у большинства форматов.
//! Поэтому здесь классический путь: промежуточный host-visible буфер (тот
//! же `buffer::upload_data`, что и у вершин), копия в device-local картинку
//! командой `vkCmdCopyBufferToImage`, и переход раскладки в обе стороны —
//! `UNDEFINED → TRANSFER_DST_OPTIMAL` (можно писать) `→ SHADER_READ_ONLY_OPTIMAL`
//! (можно читать в шейдере). Раскладка — это не формальность, а буквально
//! другой порядок байт в памяти, который выбирает драйвер под конкретный
//! GPU; читать/писать картинку в «неправильной» для операции раскладке
//! запрещено спецификацией напрямую.
//!
//! **Одноразовый командный буфер.** Копирование происходит один раз, при
//! загрузке текстуры, а не каждый кадр — поэтому вместо командного буфера
//! из общего пула, что переиспользуется в `draw_frame`, здесь свой, на одно
//! использование: аллоцируется, пишется, отправляется, и после
//! `vkQueueWaitIdle` сразу освобождается. Ждать так очередь целиком, а не
//! заводить отдельный fence, можно ровно потому, что это происходит один
//! раз при старте, а не в кадровом цикле — там такое ожидание было бы
//! катастрофой для FPS, здесь цена никого не волнует

use crate::vulkan::buffer;
use crate::vulkan::device::Device;
use crate::vulkan::ffi::*;

pub struct GpuImage {
    pub view: VkImageView,
    image: VkImage,
    memory: VkDeviceMemory,
}

impl GpuImage {
    pub fn upload_rgba8(
        device: &Device,
        memory_properties: &VkPhysicalDeviceMemoryProperties,
        command_pool: VkCommandPool,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<Self, String> {
        let mut staging = buffer::upload_data(device, memory_properties, VK_BUFFER_USAGE_TRANSFER_SRC_BIT, pixels)?;

        let (image, memory) = match create_device_local_image(device, memory_properties, width, height) {
            Ok(pair) => pair,
            Err(err) => {
                staging.destroy(device);
                return Err(err);
            }
        };

        let upload_result = record_and_submit_once(device, command_pool, |cmd| unsafe {
            transition_layout(device, cmd, image, VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL);
            copy_buffer_to_image(device, cmd, staging.handle, image, width, height);
            transition_layout(
                device,
                cmd,
                image,
                VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
            );
        });
        // Ненужен независимо от исхода: копия либо уже сделана, либо не
        // будет сделана уже никогда
        staging.destroy(device);
        if let Err(err) = upload_result {
            unsafe {
                (device.fns.destroy_image)(device.handle, image, std::ptr::null());
                (device.fns.free_memory)(device.handle, memory, std::ptr::null());
            }
            return Err(err);
        }

        let view = match create_view(device, image) {
            Ok(view) => view,
            Err(err) => {
                unsafe {
                    (device.fns.destroy_image)(device.handle, image, std::ptr::null());
                    (device.fns.free_memory)(device.handle, memory, std::ptr::null());
                }
                return Err(err);
            }
        };

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

fn create_device_local_image(
    device: &Device,
    memory_properties: &VkPhysicalDeviceMemoryProperties,
    width: u32,
    height: u32,
) -> Result<(VkImage, VkDeviceMemory), String> {
    let create_info = VkImageCreateInfo {
        s_type: VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        image_type: VK_IMAGE_TYPE_2D,
        format: VK_FORMAT_R8G8B8A8_UNORM,
        extent: VkExtent3D { width, height, depth: 1 },
        mip_levels: 1,
        array_layers: 1,
        samples: VK_SAMPLE_COUNT_1_BIT,
        tiling: VK_IMAGE_TILING_OPTIMAL,
        usage: VK_IMAGE_USAGE_TRANSFER_DST_BIT | VK_IMAGE_USAGE_SAMPLED_BIT,
        sharing_mode: VK_SHARING_MODE_EXCLUSIVE,
        queue_family_index_count: 0,
        p_queue_family_indices: std::ptr::null(),
        initial_layout: VK_IMAGE_LAYOUT_UNDEFINED,
    };
    let mut image = VkImage::NULL;
    let result = unsafe { (device.fns.create_image)(device.handle, &create_info, std::ptr::null(), &mut image) };
    if result != VK_SUCCESS {
        return Err(format!("vkCreateImage вернул {result}"));
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
    let result = unsafe { (device.fns.allocate_memory)(device.handle, &allocate_info, std::ptr::null(), &mut memory) };
    if result != VK_SUCCESS {
        unsafe {
            (device.fns.destroy_image)(device.handle, image, std::ptr::null());
        }
        return Err(format!("vkAllocateMemory (изображение) вернул {result}"));
    }

    let result = unsafe { (device.fns.bind_image_memory)(device.handle, image, memory, 0) };
    if result != VK_SUCCESS {
        unsafe {
            (device.fns.free_memory)(device.handle, memory, std::ptr::null());
            (device.fns.destroy_image)(device.handle, image, std::ptr::null());
        }
        return Err(format!("vkBindImageMemory вернул {result}"));
    }

    Ok((image, memory))
}

fn create_view(device: &Device, image: VkImage) -> Result<VkImageView, String> {
    let create_info = VkImageViewCreateInfo {
        s_type: VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        image,
        view_type: VK_IMAGE_VIEW_TYPE_2D,
        format: VK_FORMAT_R8G8B8A8_UNORM,
        components: VkComponentMapping {
            r: VK_COMPONENT_SWIZZLE_IDENTITY,
            g: VK_COMPONENT_SWIZZLE_IDENTITY,
            b: VK_COMPONENT_SWIZZLE_IDENTITY,
            a: VK_COMPONENT_SWIZZLE_IDENTITY,
        },
        subresource_range: VkImageSubresourceRange {
            aspect_mask: VK_IMAGE_ASPECT_COLOR_BIT,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        },
    };
    let mut view = VkImageView::NULL;
    let result = unsafe { (device.fns.create_image_view)(device.handle, &create_info, std::ptr::null(), &mut view) };
    if result != VK_SUCCESS {
        return Err(format!("vkCreateImageView (текстура) вернул {result}"));
    }
    Ok(view)
}

/// Один переход раскладки — один барьер. Обе используемые здесь пары уже
/// покрывают весь путь одной картинки (записать → прочитать), заводить
/// таблицу под произвольные пары незачем: третьей паре в этом движке пока
/// неоткуда взяться
unsafe fn transition_layout(device: &Device, cmd: VkCommandBuffer, image: VkImage, old_layout: VkEnum, new_layout: VkEnum) {
    let (src_access, dst_access, src_stage, dst_stage) = match (old_layout, new_layout) {
        (VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL) => {
            (0, VK_ACCESS_TRANSFER_WRITE_BIT, VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT)
        }
        (VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL) => (
            VK_ACCESS_TRANSFER_WRITE_BIT,
            VK_ACCESS_SHADER_READ_BIT,
            VK_PIPELINE_STAGE_TRANSFER_BIT,
            VK_PIPELINE_STAGE_FRAGMENT_SHADER_BIT,
        ),
        _ => unreachable!("переход раскладки, для которого здесь не заведена пара стадий/масок доступа"),
    };

    let barrier = VkImageMemoryBarrier {
        s_type: VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER,
        p_next: std::ptr::null(),
        src_access_mask: src_access,
        dst_access_mask: dst_access,
        old_layout,
        new_layout,
        src_queue_family_index: VK_QUEUE_FAMILY_IGNORED,
        dst_queue_family_index: VK_QUEUE_FAMILY_IGNORED,
        image,
        subresource_range: VkImageSubresourceRange {
            aspect_mask: VK_IMAGE_ASPECT_COLOR_BIT,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        },
    };

    unsafe {
        (device.fns.cmd_pipeline_barrier)(
            cmd,
            src_stage,
            dst_stage,
            0,
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            1,
            &barrier,
        );
    }
}

unsafe fn copy_buffer_to_image(device: &Device, cmd: VkCommandBuffer, buffer: VkBuffer, image: VkImage, width: u32, height: u32) {
    let region = VkBufferImageCopy {
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image_subresource: VkImageSubresourceLayers {
            aspect_mask: VK_IMAGE_ASPECT_COLOR_BIT,
            mip_level: 0,
            base_array_layer: 0,
            layer_count: 1,
        },
        image_offset: VkOffset3D { x: 0, y: 0, z: 0 },
        image_extent: VkExtent3D { width, height, depth: 1 },
    };
    unsafe {
        (device.fns.cmd_copy_buffer_to_image)(cmd, buffer, image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &region);
    }
}

/// Аллоцирует командный буфер, даёт вызывающей стороне записать в него что
/// угодно через замыкание, отправляет и ждёт выполнения — и освобождает
/// буфер обратно в пул. Нужен ровно один раз при загрузке текстуры, поэтому
/// не завязан на `FrameSync`: тот рассчитан на кадровый цикл (семафоры,
/// fence без ожидания очереди целиком), а здесь как раз ожидание очереди —
/// самый простой вариант, раз это не кадровый цикл
fn record_and_submit_once(
    device: &Device,
    command_pool: VkCommandPool,
    record: impl FnOnce(VkCommandBuffer),
) -> Result<(), String> {
    let allocate_info = VkCommandBufferAllocateInfo {
        s_type: VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
        p_next: std::ptr::null(),
        command_pool,
        level: VK_COMMAND_BUFFER_LEVEL_PRIMARY,
        command_buffer_count: 1,
    };
    let mut cmd = VkCommandBuffer::NULL;
    let result = unsafe { (device.fns.allocate_command_buffers)(device.handle, &allocate_info, &mut cmd) };
    if result != VK_SUCCESS {
        return Err(format!("vkAllocateCommandBuffers (загрузка текстуры) вернул {result}"));
    }

    let begin_info = VkCommandBufferBeginInfo {
        s_type: VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
        p_next: std::ptr::null(),
        flags: VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT,
        p_inheritance_info: std::ptr::null(),
    };
    unsafe {
        (device.fns.begin_command_buffer)(cmd, &begin_info);
    }

    record(cmd);

    let result = unsafe { (device.fns.end_command_buffer)(cmd) };
    if result != VK_SUCCESS {
        unsafe {
            (device.fns.free_command_buffers)(device.handle, command_pool, 1, &cmd);
        }
        return Err(format!("vkEndCommandBuffer (загрузка текстуры) вернул {result}"));
    }

    let submit_info = VkSubmitInfo {
        s_type: VK_STRUCTURE_TYPE_SUBMIT_INFO,
        p_next: std::ptr::null(),
        wait_semaphore_count: 0,
        p_wait_semaphores: std::ptr::null(),
        p_wait_dst_stage_mask: std::ptr::null(),
        command_buffer_count: 1,
        p_command_buffers: &cmd,
        signal_semaphore_count: 0,
        p_signal_semaphores: std::ptr::null(),
    };
    let submit_result = unsafe { (device.fns.queue_submit)(device.queue, 1, &submit_info, VkFence::NULL) };
    if submit_result == VK_SUCCESS {
        unsafe {
            (device.fns.queue_wait_idle)(device.queue);
        }
    }
    unsafe {
        (device.fns.free_command_buffers)(device.handle, command_pool, 1, &cmd);
    }
    if submit_result != VK_SUCCESS {
        return Err(format!("vkQueueSubmit (загрузка текстуры) вернул {submit_result}"));
    }
    Ok(())
}
