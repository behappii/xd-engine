//! Софтверный 3D-растеризатор.
//!
//! Крейт чисто библиотечный: демо-сцена лежит в `examples/demo.rs` и
//! подключает движок так же, как это сделал бы чужой проект — через публичный
//! API. Благодаря этому движок одинаково доступен интеграционным тестам в
//! `tests/`, примерам в `examples/` и doc-тестам, а чего не хватит любому из
//! них — то дырка в публичном API.

pub mod app;
pub mod clipping;
pub mod config;
pub mod fps_counter;
pub mod math;
pub mod renderer;
pub mod scene;
pub mod texture;
