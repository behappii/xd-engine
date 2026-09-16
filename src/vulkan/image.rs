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
    /// Сколько уровней в пирамиде. Единица — пирамиды нет. Нужно наружу,
    /// потому что сэмплер обязан знать тот же предел (`max_lod` в
    /// `sampler.rs`): разойдутся — либо уровни не читаются вовсе, либо
    /// сэмплер уходит за последний
    pub mip_levels: u32,
    image: VkImage,
    memory: VkDeviceMemory,
}

/// Сколько уровней помещается в пирамиду картинки такого размера.
///
/// Каждый следующий уровень вдвое мельче, последний — ровно один тексель,
/// поэтому это двоичный логарифм БОЛЬШЕЙ стороны плюс сам нулевой уровень.
/// Большей, а не меньшей: у картинки 64×8 деление продолжается и после того,
/// как короткая сторона упёрлась в единицу, — она просто перестаёт делиться
/// (`(x / 2).max(1)` в `generate_mipmaps`), а длинная идёт до конца.
///
/// Та же формула, что у `Texture::with_mipmaps` на CPU-пути, и это не
/// совпадение: уровень, выбранный по отпечатку, обязан означать на обоих
/// путях одно и то же
pub fn mip_level_count(width: u32, height: u32) -> u32 {
    32 - width.max(height).max(1).leading_zeros()
}

impl GpuImage {
    pub fn upload_rgba8(
        device: &Device,
        memory_properties: &VkPhysicalDeviceMemoryProperties,
        command_pool: VkCommandPool,
        width: u32,
        height: u32,
        pixels: &[u8],
        mip_levels: u32,
    ) -> Result<Self, String> {
        // Ноль уровней Vulkan не примет, а единица — это «пирамиды нет»
        let mip_levels = mip_levels.max(1);

        let mut staging = buffer::upload_data(device, memory_properties, VK_BUFFER_USAGE_TRANSFER_SRC_BIT, pixels)?;

        let (image, memory) = match create_device_local_image(device, memory_properties, width, height, mip_levels) {
            Ok(pair) => pair,
            Err(err) => {
                staging.destroy(device);
                return Err(err);
            }
        };

        let upload_result = record_and_submit_once(device, command_pool, |cmd| unsafe {
            // Переводим СРАЗУ ВСЕ уровни: с CPU приезжает только нулевой, но
            // остальные тоже должны оказаться в `TRANSFER_DST` — блит будет
            // писать в них, а раскладка `UNDEFINED` для приёмника копирования
            // не годится
            transition_layout(
                device,
                cmd,
                image,
                VK_IMAGE_LAYOUT_UNDEFINED,
                VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                0,
                mip_levels,
            );
            copy_buffer_to_image(device, cmd, staging.handle, image, width, height);

            if mip_levels > 1 {
                // Достраивает пирамиду И доводит ВСЕ уровни до
                // `SHADER_READ_ONLY_OPTIMAL` — отдельного перехода после него
                // не нужно
                generate_mipmaps(device, cmd, image, width, height, mip_levels);
            } else {
                transition_layout(
                    device,
                    cmd,
                    image,
                    VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                    VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
                    0,
                    1,
                );
            }
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

        let view = match create_view(device, image, mip_levels) {
            Ok(view) => view,
            Err(err) => {
                unsafe {
                    (device.fns.destroy_image)(device.handle, image, std::ptr::null());
                    (device.fns.free_memory)(device.handle, memory, std::ptr::null());
                }
                return Err(err);
            }
        };

        Ok(Self { view, mip_levels, image, memory })
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
    mip_levels: u32,
) -> Result<(VkImage, VkDeviceMemory), String> {
    let create_info = VkImageCreateInfo {
        s_type: VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        image_type: VK_IMAGE_TYPE_2D,
        format: VK_FORMAT_R8G8B8A8_UNORM,
        extent: VkExtent3D { width, height, depth: 1 },
        mip_levels,
        array_layers: 1,
        samples: VK_SAMPLE_COUNT_1_BIT,
        tiling: VK_IMAGE_TILING_OPTIMAL,
        // `TRANSFER_SRC` нужен не для загрузки, а для построения пирамиды:
        // блит читает уровень N-1 той же самой картинки, в которую пишет
        // уровень N. То есть картинка одновременно и источник, и приёмник —
        // забыть этот бит значит получить ошибку валидации на первом же блите
        usage: VK_IMAGE_USAGE_TRANSFER_DST_BIT | VK_IMAGE_USAGE_TRANSFER_SRC_BIT | VK_IMAGE_USAGE_SAMPLED_BIT,
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

fn create_view(device: &Device, image: VkImage, mip_levels: u32) -> Result<VkImageView, String> {
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
            // ВСЕ уровни, а не один: view — это то, через что сэмплер видит
            // картинку, и уровни, не попавшие сюда, для шейдера не
            // существуют. Оставить единицу — значит построить пирамиду и не
            // дать ею воспользоваться
            level_count: mip_levels,
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

/// Один переход раскладки — один барьер, и переводится не вся картинка
/// целиком, а ДИАПАЗОН УРОВНЕЙ `base_mip_level..base_mip_level + level_count`.
///
/// Диапазон здесь не про общность на будущее: при построении пирамиды
/// картинка законно находится в РАЗНЫХ раскладках одновременно — уровень,
/// с которого сейчас читает блит, в `TRANSFER_SRC`, тот, в который он пишет,
/// в `TRANSFER_DST`, а уже готовые уровни переведены в `SHADER_READ_ONLY`.
/// Раскладка в Vulkan — свойство подресурса, а не картинки, и ровно этим
/// свойством тут и пользуются
unsafe fn transition_layout(
    device: &Device,
    cmd: VkCommandBuffer,
    image: VkImage,
    old_layout: VkEnum,
    new_layout: VkEnum,
    base_mip_level: u32,
    level_count: u32,
) {
    let (src_access, dst_access, src_stage, dst_stage) = match (old_layout, new_layout) {
        (VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL) => {
            (0, VK_ACCESS_TRANSFER_WRITE_BIT, VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT)
        }
        // Уровень дописан копированием или блитом — теперь с него читает
        // следующий блит. Обе стороны на стадии передачи: это чтение-после-
        // записи ВНУТРИ одной и той же стадии, и без барьера блит мог бы
        // прочитать уровень раньше, чем тот дописан
        (VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL) => (
            VK_ACCESS_TRANSFER_WRITE_BIT,
            VK_ACCESS_TRANSFER_READ_BIT,
            VK_PIPELINE_STAGE_TRANSFER_BIT,
            VK_PIPELINE_STAGE_TRANSFER_BIT,
        ),
        (VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL) => (
            VK_ACCESS_TRANSFER_WRITE_BIT,
            VK_ACCESS_SHADER_READ_BIT,
            VK_PIPELINE_STAGE_TRANSFER_BIT,
            VK_PIPELINE_STAGE_FRAGMENT_SHADER_BIT,
        ),
        // Уровень отдал себя следующему и больше в передаче не участвует —
        // остаётся только читать его шейдером
        (VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL) => (
            VK_ACCESS_TRANSFER_READ_BIT,
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
            base_mip_level,
            level_count,
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

/// Достраивает пирамиду блитами: уровень N получается из уровня N-1,
/// ужатого вдвое самой видеокартой.
///
/// **Почему блитом, а не расчётом на CPU.** Пирамиду умеет строить и
/// `Texture::with_mipmaps` — на CPU-пути работает именно она. Но грузить её
/// уровни по одному значило бы гонять через шину то, что видеокарта сделает
/// у себя: исходная картинка УЖЕ лежит в её памяти, и всё построение — это
/// несколько команд без единого байта по шине. Так это делают настоящие
/// движки.
///
/// **Цепочка, а не «все из нулевого».** Каждый уровень строится из
/// ПРЕДЫДУЩЕГО, и это не экономия: блит усредняет небольшую окрестность, а не
/// всю область, которую уровень представляет. Ужать 512 в 8 одним блитом —
/// значит прочитать из исходной картинки лишь горстку текселей и выбросить
/// остальные, то есть получить на дальнем плане ту же рябь, ради избавления
/// от которой пирамида и строится. Цепочка же усредняет каждый тексель ровно
/// один раз на уровень, и вклад в итог получают все.
///
/// На выходе ВСЕ уровни оставлены в `SHADER_READ_ONLY_OPTIMAL` — дополнять
/// переходами снаружи не нужно
unsafe fn generate_mipmaps(
    device: &Device,
    cmd: VkCommandBuffer,
    image: VkImage,
    width: u32,
    height: u32,
    mip_levels: u32,
) {
    let mut level_width = width as i32;
    let mut level_height = height as i32;

    for level in 1..mip_levels {
        unsafe {
            transition_layout(
                device,
                cmd,
                image,
                VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                level - 1,
                1,
            );
        }

        // `.max(1)`: сторона, добравшаяся до единицы, дальше не делится, а
        // длинная продолжает — так у картинки 64x8 последние уровни выходят
        // 4x1, 2x1, 1x1. Без зажима сторона стала бы нулевой, а картинки
        // нулевого размера не бывает
        let next_width = (level_width / 2).max(1);
        let next_height = (level_height / 2).max(1);

        let blit = VkImageBlit {
            src_subresource: VkImageSubresourceLayers {
                aspect_mask: VK_IMAGE_ASPECT_COLOR_BIT,
                mip_level: level - 1,
                base_array_layer: 0,
                layer_count: 1,
            },
            // Два УГЛА области, а не позиция с размером. Разница углов
            // источника вдвое больше, чем у приёмника, — в этом и состоит
            // ужатие; фильтрацию между ними делает сам блит
            src_offsets: [VkOffset3D { x: 0, y: 0, z: 0 }, VkOffset3D { x: level_width, y: level_height, z: 1 }],
            dst_subresource: VkImageSubresourceLayers {
                aspect_mask: VK_IMAGE_ASPECT_COLOR_BIT,
                mip_level: level,
                base_array_layer: 0,
                layer_count: 1,
            },
            dst_offsets: [VkOffset3D { x: 0, y: 0, z: 0 }, VkOffset3D { x: next_width, y: next_height, z: 1 }],
        };

        unsafe {
            (device.fns.cmd_blit_image)(
                cmd,
                image,
                VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                image,
                VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                1,
                &blit,
                // Линейный, а не ближайший сосед: усреднение — это весь смысл
                // уровня. С `VK_FILTER_NEAREST` пирамида построилась бы, но
                // каждый уровень брал бы из четвёрки текселей один, и рябь
                // вернулась бы ровно в том виде, от которого уровни лечат.
                // Формат обязан уметь линейную фильтрацию — проверено до
                // вызова, см. `gpu_assets`
                VK_FILTER_LINEAR,
            );

            // Уровень-источник свою работу сделал: переводим его туда, где он
            // и проведёт остаток жизни
            transition_layout(
                device,
                cmd,
                image,
                VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
                level - 1,
                1,
            );
        }

        level_width = next_width;
        level_height = next_height;
    }

    // Последний уровень источником не был и остался в `TRANSFER_DST` — его
    // переводим отдельно. Забыть эту строку легко, а симптом указывал бы куда
    // угодно, только не сюда: самый мелкий уровень читается лишь у далёкой
    // поверхности
    unsafe {
        transition_layout(
            device,
            cmd,
            image,
            VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
            VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
            mip_levels - 1,
            1,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_square_texture_halves_down_to_a_single_texel() {
        // 64 → 32 → 16 → 8 → 4 → 2 → 1, то есть семь уровней вместе с нулевым
        assert_eq!(mip_level_count(64, 64), 7);
        assert_eq!(mip_level_count(1, 1), 1);
        assert_eq!(mip_level_count(2, 2), 2);
    }

    #[test]
    fn the_count_follows_the_longer_side() {
        // Короткая сторона упирается в единицу и дальше не делится, а длинная
        // продолжает: у 64x8 последние уровни выходят 4x1, 2x1, 1x1. Считай мы
        // по КОРОТКОЙ — вышло бы четыре уровня вместо семи, и сэмплеру не
        // хватило бы как раз самых мелких, тех что читаются у горизонта
        assert_eq!(mip_level_count(64, 8), 7);
        assert_eq!(mip_level_count(8, 64), 7);
    }

    #[test]
    fn a_non_power_of_two_side_rounds_down() {
        // 100 → 50 → 25 → 12 → 6 → 3 → 1 — семь уровней, как и у 64:
        // добавочный появляется только на следующей степени двойки.
        // Целочисленное деление по пути теряет нечётный тексель, и это
        // нормально — так же округляет `Texture::with_mipmaps` на CPU-пути
        assert_eq!(mip_level_count(100, 100), 7);
        assert_eq!(mip_level_count(128, 128), 8);
    }
}
