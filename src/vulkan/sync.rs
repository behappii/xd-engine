//! Синхронизация одного кадра между CPU и GPU.
//!
//! Vulkan не ждёт ничего сам — если не сказать явно, CPU может начать
//! записывать команды следующего кадра поверх буфера, который GPU ещё
//! читает, или показать зрителю картинку, которая ещё не дорисована.
//! Нужны две разные примитива, а не одна, потому что они ждут РАЗНОЕ:
//! семафоры — это GPU ждёт GPU (одна очередь команд ждёт, пока другая
//! стадия того же кадра закончит), а fence — это CPU ждёт GPU (наш поток
//! останавливается, пока видеокарта не закончит) . Заменить одно другим
//! нельзя: `vkQueueSubmit` умеет ждать только семафор, а
//! `vkWaitForFences` — единственный способ CPU вообще узнать, что GPU
//! закончил.
//!
//! **Один кадр в полёте, не два-три.** Настоящие движки заводят несколько
//! комплектов синхронизации (double/triple buffering), чтобы CPU готовил
//! следующий кадр, пока GPU ещё рисует предыдущий. Здесь — сознательное
//! упрощение Фазы 1: CPU ждёт GPU целиком перед тем, как начать
//! следующий кадр. Для одного зашитого в шейдер треугольника разницы не
//! видно, а многокомплектная версия — понятный кандидат для одной из
//! следующих фаз, когда будет что параллелить.
//!
//! **Но `render_finished` всё равно нужен СВОЙ на каждую картинку swapchain
//! — и это не задел на будущее, а починка настоящей ошибки.** Рассуждение,
//! на котором держался один общий семафор, звучало убедительно: кадр в
//! полёте один, `vkWaitForFences` в начале `draw_frame` дожидается
//! предыдущего, значит переиспользовать нечего. Оно неверно, и вот где:
//! fence закрывает завершение ОТРИСОВКИ, а показ — отдельная операция,
//! которая ждёт `render_finished` уже после того, как fence сработал.
//! Картинок в swapchain больше одной, поэтому следующий кадр берёт другую
//! картинку и сигналит тот же семафор, пока предыдущий показ ещё может его
//! держать.
//!
//! Поймал это слой валидации сразу после установки Vulkan SDK
//! (`VUID-vkQueueSubmit-pSignalSemaphores-00067`), и до него ошибка ничем
//! себя не проявляла — картинка была правильной. Лечится ровно тем, что
//! советует само сообщение: семафор на каждую картинку, индексируемый
//! номером той картинки, которую вернул `vkAcquireNextImageKHR`.
//!
//! `image_available` при этом остаётся ОДИН, и это не непоследовательность:
//! его ждёт `vkQueueSubmit`, то есть ровно та работа, завершение которой
//! fence и стережёт. К следующему `vkAcquireNextImageKHR` он гарантированно
//! свободен. Его пришлось бы размножать по числу кадров в полёте, а их
//! по-прежнему один

use crate::vulkan::device::Device;
use crate::vulkan::ffi::*;

pub struct FrameSync {
    pub image_available: VkSemaphore,
    /// По одному на КАРТИНКУ swapchain, а не на кадр в полёте — индексируется
    /// номером картинки от `vkAcquireNextImageKHR` (см. doc-комментарий модуля)
    pub render_finished: Vec<VkSemaphore>,
    pub in_flight: VkFence,
}

impl FrameSync {
    /// `swapchain_image_count` — сколько картинок в swapchain'е, столько же
    /// будет и семафоров `render_finished`
    pub fn new(device: &Device, swapchain_image_count: usize) -> Result<Self, String> {
        let semaphore_info = VkSemaphoreCreateInfo { s_type: VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO, p_next: std::ptr::null(), flags: 0 };

        let mut image_available = VkSemaphore::NULL;
        let result = unsafe {
            (device.fns.create_semaphore)(device.handle, &semaphore_info, std::ptr::null(), &mut image_available)
        };
        if result != VK_SUCCESS {
            return Err(format!("vkCreateSemaphore (image_available) вернул {result}"));
        }

        let mut render_finished = Vec::with_capacity(swapchain_image_count);
        for index in 0..swapchain_image_count {
            let mut semaphore = VkSemaphore::NULL;
            let result = unsafe {
                (device.fns.create_semaphore)(device.handle, &semaphore_info, std::ptr::null(), &mut semaphore)
            };
            if result != VK_SUCCESS {
                // Разматываем то, что уже создано: у Vulkan нет деструкторов,
                // и ранний выход отсюда — единственное место, где семафоры
                // могли бы остаться висеть до конца процесса. Тот же порядок
                // уборки, что уже принят в `buffer.rs`/`image.rs`/`depth.rs`
                unsafe {
                    for &created in &render_finished {
                        (device.fns.destroy_semaphore)(device.handle, created, std::ptr::null());
                    }
                    (device.fns.destroy_semaphore)(device.handle, image_available, std::ptr::null());
                }
                return Err(format!("vkCreateSemaphore (render_finished #{index}) вернул {result}"));
            }
            render_finished.push(semaphore);
        }

        // SIGNALED сразу при создании: первый кадр не ждёт результат
        // несуществующего предыдущего. Без этого флага первый же
        // `vkWaitForFences` в `context.rs` завис бы навсегда — ждать
        // нечего, потому что рисовать ещё не начинали
        let fence_info = VkFenceCreateInfo {
            s_type: VK_STRUCTURE_TYPE_FENCE_CREATE_INFO,
            p_next: std::ptr::null(),
            flags: VK_FENCE_CREATE_SIGNALED_BIT,
        };
        let mut in_flight = VkFence::NULL;
        let result = unsafe { (device.fns.create_fence)(device.handle, &fence_info, std::ptr::null(), &mut in_flight) };
        if result != VK_SUCCESS {
            unsafe {
                for &created in &render_finished {
                    (device.fns.destroy_semaphore)(device.handle, created, std::ptr::null());
                }
                (device.fns.destroy_semaphore)(device.handle, image_available, std::ptr::null());
            }
            return Err(format!("vkCreateFence вернул {result}"));
        }

        Ok(Self { image_available, render_finished, in_flight })
    }

    /// Не `Drop` — по той же причине, что у `Swapchain`: уничтожение
    /// требует device-таблицу функций, а порядок закрытия у Vulkan-объектов
    /// прописан явно в одном месте (`Renderer::drop` в `context.rs`), а не
    /// разбросан по мелким деструкторам
    pub fn destroy(&mut self, device: &Device) {
        unsafe {
            (device.fns.destroy_semaphore)(device.handle, self.image_available, std::ptr::null());
            for &semaphore in &self.render_finished {
                (device.fns.destroy_semaphore)(device.handle, semaphore, std::ptr::null());
            }
            (device.fns.destroy_fence)(device.handle, self.in_flight, std::ptr::null());
        }
        self.render_finished.clear();
    }
}
