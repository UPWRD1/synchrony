//! A UI application that uses [`libautark`] to create a DAW.
use libautark::demo;

#[tokio::main(flavor = "multi_thread")]
pub async fn main() {
    // if cfg!(debug_assertions) {
    //     // Start the console subscriber
    console_subscriber::init();
    // }

    let _handle = tokio::runtime::Handle::current();

    demo().await;
}
