//! Descriptor set — впервые в этом движке: до сих пор вся динамика кадра
//! шла через push-constant (MVP, матрица нормалей), а картинка размером с
//! текстуру туда не влезает и не должна — push-constant гарантированно
//! доступен только на 128 байт (см. `pipeline::PushConstants`). Один
//! биндинг, `VkSampler`+`VkImageView` вместе
//! (`VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER`) — ровно то, что в шейдере
//! (`shader.rs`) читается одной переменной `OpTypeSampledImage`, а не
//! раздельными «текстура» и «сэмплер».
//!
//! **Почему layout и сам набор — разные функции.** `VkDescriptorSetLayout`
//! нужен уже при сборке `VkPipelineLayoutCreateInfo` (`pipeline.rs`) — то
//! есть до того, как появилась настоящая картинка и сэмплер, которые в этот
//! набор запишутся. Сам набор (`VkDescriptorSet`) собирается позже, когда
//! `VkImageView`/`VkSampler` уже существуют. Слить оба шага в один
//! конструктор значило бы искусственно тянуть загрузку текстуры выше по
//! коду, к месту, где строится пайплайн, — хотя по сути это два независимых
//! момента: layout — это форма данных, набор — сами данные

use crate::vulkan::device::Device;
use crate::vulkan::ffi::*;

/// Форма единственного биндинга: один `COMBINED_IMAGE_SAMPLER`, читаемый
/// только фрагментным шейдером. Живёт до конца жизни рендерера — уничтожает
/// его `context.rs` вместе с pipeline layout, который на него ссылается
pub fn create_set_layout(device: &Device) -> Result<VkDescriptorSetLayout, String> {
    let binding = VkDescriptorSetLayoutBinding {
        binding: 0,
        descriptor_type: VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER,
        descriptor_count: 1,
        stage_flags: VK_SHADER_STAGE_FRAGMENT_BIT,
        p_immutable_samplers: std::ptr::null(),
    };
    let create_info = VkDescriptorSetLayoutCreateInfo {
        s_type: VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        binding_count: 1,
        p_bindings: &binding,
    };
    let mut set_layout = VkDescriptorSetLayout::NULL;
    let result = unsafe {
        (device.fns.create_descriptor_set_layout)(device.handle, &create_info, std::ptr::null(), &mut set_layout)
    };
    if result != VK_SUCCESS {
        return Err(format!("vkCreateDescriptorSetLayout вернул {result}"));
    }
    Ok(set_layout)
}

/// Пул на один набор (текстур у нас пока одна — Фаза 5 умножит инстансы, не
/// обязательно наборы: сэмплер на инстанс не завязан) и сам набор,
/// заполненный ссылкой на конкретные `VkImageView`+`VkSampler`
pub struct Descriptor {
    pool: VkDescriptorPool,
    pub set: VkDescriptorSet,
}

impl Descriptor {
    pub fn new(
        device: &Device,
        set_layout: VkDescriptorSetLayout,
        view: VkImageView,
        sampler: VkSampler,
    ) -> Result<Self, String> {
        let pool_size = VkDescriptorPoolSize { descriptor_type: VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER, descriptor_count: 1 };
        let pool_info = VkDescriptorPoolCreateInfo {
            s_type: VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO,
            p_next: std::ptr::null(),
            flags: 0,
            max_sets: 1,
            pool_size_count: 1,
            p_pool_sizes: &pool_size,
        };
        let mut pool = VkDescriptorPool::NULL;
        let result =
            unsafe { (device.fns.create_descriptor_pool)(device.handle, &pool_info, std::ptr::null(), &mut pool) };
        if result != VK_SUCCESS {
            return Err(format!("vkCreateDescriptorPool вернул {result}"));
        }

        let allocate_info = VkDescriptorSetAllocateInfo {
            s_type: VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO,
            p_next: std::ptr::null(),
            descriptor_pool: pool,
            descriptor_set_count: 1,
            p_set_layouts: &set_layout,
        };
        let mut set = VkDescriptorSet::NULL;
        let result = unsafe { (device.fns.allocate_descriptor_sets)(device.handle, &allocate_info, &mut set) };
        if result != VK_SUCCESS {
            unsafe {
                (device.fns.destroy_descriptor_pool)(device.handle, pool, std::ptr::null());
            }
            return Err(format!("vkAllocateDescriptorSets вернул {result}"));
        }

        let image_info = VkDescriptorImageInfo { sampler, image_view: view, image_layout: VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL };
        let write = VkWriteDescriptorSet {
            s_type: VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
            p_next: std::ptr::null(),
            dst_set: set,
            dst_binding: 0,
            dst_array_element: 0,
            descriptor_count: 1,
            descriptor_type: VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER,
            p_image_info: &image_info,
            p_buffer_info: std::ptr::null(),
            p_texel_buffer_view: std::ptr::null(),
        };
        unsafe {
            (device.fns.update_descriptor_sets)(device.handle, 1, &write, 0, std::ptr::null());
        }

        Ok(Self { pool, set })
    }

    /// Уничтожает только пул (и вместе с ним, неявно, выделенный из него
    /// набор) — не layout: тот создан снаружи (`create_set_layout`) и
    /// уничтожается вместе с pipeline layout в `context.rs`, симметрично
    /// тому, как он там же и создавался раньше картинки
    pub fn destroy(&self, device: &Device) {
        unsafe {
            (device.fns.destroy_descriptor_pool)(device.handle, self.pool, std::ptr::null());
        }
    }
}
