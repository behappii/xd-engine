//! Буферы GPU-памяти: вершинный и индексный буфер куба заводятся через один
//! и тот же путь — `upload_data`.
//!
//! **Без staging-буфера.** Настоящие движки грузят геометрию в ДВА шага:
//! данные сначала копируются в маленький host-visible буфер, а из него —
//! командой `vkCmdCopyBuffer` в `DEVICE_LOCAL` память, к которой у CPU вообще
//! нет доступа, зато у GPU к ней самая быстрая шина. Здесь память сразу
//! `HOST_VISIBLE | HOST_COHERENT` — CPU пишет прямо в неё, GPU читает оттуда
//! же. Для куба на 36 вершин разница не измерима, а лишний командный буфер
//! и барьер только отвлекали бы от того, что Фаза 2 на самом деле проверяет:
//! что вершины из `scene::Mesh` вообще доезжают до GPU. Staging — кандидат
//! на одну из следующих фаз, когда геометрии станет действительно много

use crate::vulkan::device::Device;
use crate::vulkan::ffi::*;

pub struct Buffer {
    pub handle: VkBuffer,
    memory: VkDeviceMemory,
}

impl Buffer {
    pub fn destroy(&mut self, device: &Device) {
        unsafe {
            (device.fns.destroy_buffer)(device.handle, self.handle, std::ptr::null());
            (device.fns.free_memory)(device.handle, self.memory, std::ptr::null());
        }
        self.handle = VkBuffer::NULL;
        self.memory = VkDeviceMemory::NULL;
    }
}

/// Создаёт буфер нужного назначения (`usage`), заполняет его байтами `data`
/// и возвращает готовый к использованию `Buffer`.
///
/// Дженерик по `T`, а не `&[u8]`: вызывающая сторона отдаёт `&[GpuVertex]`
/// или `&[u32]` как есть, не сериализуя вручную — `T: Copy` гарантирует, что
/// у типа нет ничего похожего на `Vec`/указатель внутри (копировать байты
/// такого значения было бы уже неверно), а размер данных считается из
/// `size_of::<T>()`, а не угадывается снаружи
pub fn upload_data<T: Copy>(
    device: &Device,
    memory_properties: &VkPhysicalDeviceMemoryProperties,
    usage: VkFlags,
    data: &[T],
) -> Result<Buffer, String> {
    let size = std::mem::size_of_val(data) as VkDeviceSize;

    let create_info = VkBufferCreateInfo {
        s_type: VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        size,
        usage,
        sharing_mode: VK_SHARING_MODE_EXCLUSIVE,
        queue_family_index_count: 0,
        p_queue_family_indices: std::ptr::null(),
    };
    let mut handle = VkBuffer::NULL;
    let result = unsafe { (device.fns.create_buffer)(device.handle, &create_info, std::ptr::null(), &mut handle) };
    if result != VK_SUCCESS {
        return Err(format!("vkCreateBuffer вернул {result}"));
    }

    let mut requirements = VkMemoryRequirements::default();
    unsafe {
        (device.fns.get_buffer_memory_requirements)(device.handle, handle, &mut requirements);
    }

    let required_properties = VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT;
    let memory_type_index = match find_memory_type(memory_properties, requirements.memory_type_bits, required_properties)
    {
        Ok(index) => index,
        Err(err) => {
            unsafe {
                (device.fns.destroy_buffer)(device.handle, handle, std::ptr::null());
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
            (device.fns.destroy_buffer)(device.handle, handle, std::ptr::null());
        }
        return Err(format!("vkAllocateMemory вернул {result}"));
    }

    let result = unsafe { (device.fns.bind_buffer_memory)(device.handle, handle, memory, 0) };
    if result != VK_SUCCESS {
        unsafe {
            (device.fns.free_memory)(device.handle, memory, std::ptr::null());
            (device.fns.destroy_buffer)(device.handle, handle, std::ptr::null());
        }
        return Err(format!("vkBindBufferMemory вернул {result}"));
    }

    // HOST_COHERENT избавляет от vkFlushMappedMemoryRanges: память и так
    // видна GPU сразу после записи, без явной синхронизации кеша
    let mut mapped: *mut std::ffi::c_void = std::ptr::null_mut();
    let result = unsafe { (device.fns.map_memory)(device.handle, memory, 0, size, 0, &mut mapped) };
    if result != VK_SUCCESS {
        unsafe {
            (device.fns.free_memory)(device.handle, memory, std::ptr::null());
            (device.fns.destroy_buffer)(device.handle, handle, std::ptr::null());
        }
        return Err(format!("vkMapMemory вернул {result}"));
    }
    unsafe {
        std::ptr::copy_nonoverlapping(data.as_ptr() as *const u8, mapped as *mut u8, size as usize);
        (device.fns.unmap_memory)(device.handle, memory);
    }

    Ok(Buffer { handle, memory })
}

/// Ищет индекс типа памяти, подходящий И по маске (`type_filter`, бит `i`
/// соответствует типу `i` — так `vkGetBufferMemoryRequirements` возвращает
/// "может лежать в типах 0,2,5"), И по требуемым свойствам (`required`).
///
/// Чистая функция без единого вызова Vulkan — проверяется юнит-тестом на
/// подставном наборе типов памяти, без GPU
pub fn find_memory_type(
    properties: &VkPhysicalDeviceMemoryProperties,
    type_filter: u32,
    required: VkFlags,
) -> Result<u32, String> {
    for i in 0..properties.memory_type_count {
        let type_is_allowed = type_filter & (1 << i) != 0;
        let has_required_properties =
            properties.memory_types[i as usize].property_flags & required == required;

        if type_is_allowed && has_required_properties {
            return Ok(i);
        }
    }

    Err(format!("нет типа памяти со свойствами {required:#06x} среди допустимых по маске {type_filter:#010x}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_type(flags: VkFlags) -> VkMemoryType {
        VkMemoryType { property_flags: flags, heap_index: 0 }
    }

    fn properties_with(types: &[VkMemoryType]) -> VkPhysicalDeviceMemoryProperties {
        let mut memory_types = [VkMemoryType::default(); VK_MAX_MEMORY_TYPES];
        memory_types[..types.len()].copy_from_slice(types);
        VkPhysicalDeviceMemoryProperties {
            memory_type_count: types.len() as u32,
            memory_types,
            memory_heap_count: 0,
            memory_heaps: [VkMemoryHeap::default(); VK_MAX_MEMORY_HEAPS],
        }
    }

    const HOST_VISIBLE_COHERENT: VkFlags = VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT;

    #[test]
    fn picks_the_type_that_has_every_required_bit() {
        let props = properties_with(&[memory_type(0), memory_type(HOST_VISIBLE_COHERENT)]);
        // Маска допускает оба типа (0b11), выбрать обязаны второй — только
        // у него есть нужные свойства
        let index = find_memory_type(&props, 0b11, HOST_VISIBLE_COHERENT).unwrap();
        assert_eq!(index, 1);
    }

    #[test]
    fn a_type_the_mask_does_not_allow_is_skipped_even_with_right_properties() {
        // Тип 0 обладает нужными свойствами, но маска (бит 0 выключен)
        // говорит, что этот буфер там лежать не может
        let props = properties_with(&[memory_type(HOST_VISIBLE_COHERENT), memory_type(HOST_VISIBLE_COHERENT)]);
        let index = find_memory_type(&props, 0b10, HOST_VISIBLE_COHERENT).unwrap();
        assert_eq!(index, 1);
    }

    #[test]
    fn extra_property_bits_do_not_disqualify_a_type() {
        // DEVICE_LOCAL здесь не просили, но она не мешает: проверяем ПОДМНОЖЕСТВО
        // (`&required == required`), а не точное совпадение
        let extra = HOST_VISIBLE_COHERENT | 0x0000_0001;
        let props = properties_with(&[memory_type(extra)]);
        assert_eq!(find_memory_type(&props, 0b1, HOST_VISIBLE_COHERENT).unwrap(), 0);
    }

    #[test]
    fn no_matching_type_is_an_error_not_a_panic() {
        let props = properties_with(&[memory_type(0)]);
        assert!(find_memory_type(&props, 0b1, HOST_VISIBLE_COHERENT).is_err());
    }
}
