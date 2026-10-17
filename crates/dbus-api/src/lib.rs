use std::collections::HashMap;
use zbus::proxy;
use zbus::Variant;

pub const BUS_NAME: &str = "org.nemui.KoeInput";
pub const OBJECT_PATH: &str = "/org/nemui/KoeInput";
pub const INTERFACE: &str = "org.nemui.KoeInput.Recognizer1";
pub const ERROR_PREFIX: &str = "org.nemui.KoeInput.Error";

#[proxy(
    interface = "org.nemui.KoeInput.Recognizer1",
    default_service = "org.nemui.KoeInput",
    default_path = "/org/nemui/KoeInput"
)]
pub trait Recognizer1 {
    fn start(&self, options: HashMap<&str, Variant<'_>>) -> zbus::Result<u32>;
    fn stop(&self, session: u32) -> zbus::Result<()>;
    fn cancel(&self, session: u32) -> zbus::Result<()>;

    #[zbus(property)]
    fn ready(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn model(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn version(&self) -> zbus::Result<String>;

    #[zbus(signal)]
    fn state_changed(&self, session: u32, state: String) -> zbus::Result<()>;
    #[zbus(signal)]
    fn partial(&self, session: u32, text: String) -> zbus::Result<()>;
    #[zbus(signal, name = "Final")]
    fn final_text(&self, session: u32, text: String) -> zbus::Result<()>;
    #[zbus(signal)]
    fn error(&self, session: u32, code: String, message: String) -> zbus::Result<()>;
}
