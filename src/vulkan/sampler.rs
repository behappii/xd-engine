//! `VkSampler` — конфигурация ЧТЕНИЯ текстуры, отдельная от самой картинки
//! (`image.rs`). Собирается прямо из `Magnify`/`Minify` (`crate::texture`):
//! то, что CPU-путь эмулирует руками в `Texture::sample` (билинейные веса,
//! выбор мип-уровня, заворачивание координаты), здесь одной структурой
//! `VkSamplerCreateInfo` занимается железо.
//!
//! **Известный пробел Фазы 4.** Мип-пирамида на GPU-пути не построена:
//! `maxLod = 0` заставляет сэмплер всегда читать уровень 0, независимо от
//! `mipmapMode`. Значит `Minify::Mipmapped`/`Anisotropic` здесь падают на
//! обычную линейную фильтрацию внутри этого единственного уровня — ровно
//! то же поведение, что и `Minify::Linear`. Настоящая пирамида через
//! `vkCmdBlitImage` — материал для одной из следующих фаз, а не то, что
//! блокирует текстуры как таковые: сэмплер и картинка — ортогональные
//! решения даже на CPU-пути (см. `with_filter`/`with_mipmaps` в
//! `texture.rs`, где по той же причине разведены настройка и хранение)

use crate::texture::{Magnify, Minify};
use crate::vulkan::device::Device;
use crate::vulkan::ffi::*;

pub fn create(device: &Device, magnify: Magnify, minify: Minify) -> Result<VkSampler, String> {
    let mag_filter = match magnify {
        Magnify::Nearest => VK_FILTER_NEAREST,
        Magnify::Linear => VK_FILTER_LINEAR,
    };
    // Тот же вопрос, что решает приватный `Minify::within_level` на
    // CPU-пути (фильтрация ВНУТРИ уровня) — вызвать его отсюда нельзя, он
    // не виден за пределы `texture.rs`, поэтому то же решение просто
    // повторено явно
    let min_filter = match minify {
        Minify::Nearest => VK_FILTER_NEAREST,
        Minify::Linear | Minify::Mipmapped | Minify::Anisotropic { .. } => VK_FILTER_LINEAR,
    };

    let create_info = VkSamplerCreateInfo {
        s_type: VK_STRUCTURE_TYPE_SAMPLER_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        mag_filter,
        min_filter,
        // Роли не играет — `min_lod`/`max_lod` ниже зажимают выборку в
        // единственный существующий уровень 0 независимо от режима, но
        // NEAREST здесь честнее: он не обещает читать соседние уровни,
        // которых и нет
        mipmap_mode: VK_SAMPLER_MIPMAP_MODE_NEAREST,
        // Заворачивает, а не зажимает — та же конвенция, что у
        // `Texture::sample` на CPU-пути (см. «Выборка текселя заворачивает,
        // а не зажимает» в CLAUDE.md); ей же пользуется пол в demo-сцене,
        // домножая UV на число, чтобы получить плитку
        address_mode_u: VK_SAMPLER_ADDRESS_MODE_REPEAT,
        address_mode_v: VK_SAMPLER_ADDRESS_MODE_REPEAT,
        address_mode_w: VK_SAMPLER_ADDRESS_MODE_REPEAT,
        mip_lod_bias: 0.0,
        anisotropy_enable: VK_FALSE,
        max_anisotropy: 1.0,
        compare_enable: VK_FALSE,
        compare_op: VK_COMPARE_OP_ALWAYS,
        min_lod: 0.0,
        max_lod: 0.0,
        border_color: VK_BORDER_COLOR_INT_OPAQUE_BLACK,
        unnormalized_coordinates: VK_FALSE,
    };

    let mut sampler = VkSampler::NULL;
    let result = unsafe { (device.fns.create_sampler)(device.handle, &create_info, std::ptr::null(), &mut sampler) };
    if result != VK_SUCCESS {
        return Err(format!("vkCreateSampler вернул {result}"));
    }
    Ok(sampler)
}
