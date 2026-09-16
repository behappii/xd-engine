//! `VkSampler` — конфигурация ЧТЕНИЯ текстуры, отдельная от самой картинки
//! (`image.rs`). Собирается прямо из `Magnify`/`Minify` (`crate::texture`):
//! то, что CPU-путь эмулирует руками в `Texture::sample` (билинейные веса,
//! выбор мип-уровня, заворачивание координаты), здесь одной структурой
//! `VkSamplerCreateInfo` занимается железо.
//!
//! **Мип-уровни сэмплер теперь читает** — пирамиду строит `image.rs`
//! блитами, а здесь ей выставляется предел (`max_lod`) и способ смешивать
//! соседние уровни (`mipmap_mode`). Оба обязаны согласоваться с картинкой,
//! потому что проверять это некому — см. `create`.
//!
//! **А вот анизотропии по-прежнему нет.** `Minify::Anisotropic` здесь
//! вырождается в трилинейную фильтрацию: уровни читаются, но одной выборкой
//! вместо нескольких вдоль длинной стороны отпечатка. Включить её мало —
//! `anisotropyEnable` требует фичи устройства `samplerAnisotropy`, её надо
//! запросить при создании устройства, а потолок взять из
//! `limits.maxSamplerAnisotropy`. Это следующий шаг, а не забытая строка

use crate::texture::{Magnify, Minify};
use crate::vulkan::device::Device;
use crate::vulkan::ffi::*;

/// `mip_levels` — сколько уровней реально построено у картинки
/// (`GpuImage::mip_levels`). Единица означает, что пирамиды нет.
///
/// Число обязано совпадать с тем, что у картинки, и это не формальность:
/// сэмплер и картинка — два независимых объекта, Vulkan их согласованность
/// не проверяет. Занизишь — построенные уровни не будут читаться вовсе;
/// завысишь — сэмплер уйдёт за последний существующий
pub fn create(device: &Device, magnify: Magnify, minify: Minify, mip_levels: u32) -> Result<VkSampler, String> {
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
        // Как смешивать СОСЕДНИЕ УРОВНИ между собой — это не то же самое, что
        // `min_filter`, который фильтрует ВНУТРИ уровня. `LINEAR` здесь плюс
        // `LINEAR` там — это и есть трилинейная фильтрация, то же самое, что
        // делает `Minify::Mipmapped` на CPU-пути: без смешивания между
        // уровнями граница их смены видна на полу отчётливой полосой.
        //
        // Когда пирамиды нет, честнее `NEAREST`: он не обещает читать
        // соседние уровни, которых не существует
        mipmap_mode: if mip_levels > 1 {
            VK_SAMPLER_MIPMAP_MODE_LINEAR
        } else {
            VK_SAMPLER_MIPMAP_MODE_NEAREST
        },
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
        // Верхняя граница уровня, ЗА который сэмплеру нельзя. Ровно число
        // уровней, а не `mip_levels - 1`: граница исключающая, и `- 1`
        // отрезал бы самый мелкий уровень — тот, что читается у горизонта,
        // то есть ровно там, где пирамида и нужна
        max_lod: mip_levels as f32,
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
