use futures_util::stream::TryStreamExt;
use image::{RgbImage, RgbaImage};
use std::collections::HashMap;
use zbus::{Connection, MessageStream, zvariant::OwnedValue};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Connect to the session bus
    let connection = Connection::session().await?;

    // 2. Become a monitor to eavesdrop on all method calls to the Notifications interface
    let rules = &["type='method_call',interface='org.freedesktop.Notifications',member='Notify'"];
    connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus.Monitoring"),
            "BecomeMonitor",
            &(rules as &[&str], 0u32),
        )
        .await?;

    println!("Listening for Discord notifications on D-Bus...");

    // 3. Listen to the stream of messages
    let mut stream = MessageStream::from(connection);

    while let Some(msg) = stream.try_next().await? {
        // We only care about MethodCall messages
        if msg.header().message_type() != zbus::message::Type::MethodCall {
            continue;
        }

        // The D-Bus signature for the 'Notify' method is (susssasa{sv}i)
        type NotifyArgs = (
            String,                      // 0: app_name
            u32,                         // 1: replaces_id
            String,                      // 2: app_icon
            String,                      // 3: summary
            String,                      // 4: body
            Vec<String>,                 // 5: actions
            HashMap<String, OwnedValue>, // 6: hints
            i32,                         // 7: timeout
        );

        let body = msg.body();
        let args: Result<NotifyArgs, _> = body.deserialize();

        if let Ok(args) = args {
            let app_name = args.0;
            let summary = args.3;
            let hints = args.6;

            // Filter for Discord notifications only
            if app_name.to_lowercase() == "discord" {
                println!("\n[Discord] Notification Summary: {}", summary);

                // Check standard keys for image payload inside the hints dictionary
                let image_data_val = hints
                    .get("image-data")
                    .or_else(|| hints.get("image_data"))
                    .or_else(|| hints.get("icon_data"));

                if let Some(val) = image_data_val {
                    // Desktop Notifications Spec image data signature: (iiibiiay)
                    type ImageData = (i32, i32, i32, bool, i32, i32, Vec<u8>);

                    let parsed_image: Result<ImageData, _> = val.clone().try_into();

                    if let Ok(image_data) = parsed_image {
                        let (width, height, _rowstride, has_alpha, _bits, channels, data) =
                            image_data;

                        println!(
                            " -> Found image data: {}x{} pixels, {} channels",
                            width, height, channels
                        );

                        // Define a unique filename
                        let timestamp = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs();
                        let filename = format!("images/discord_img_{}.png", timestamp);

                        // Parse the raw bytes back into an image file
                        let width = width as u32;
                        let height = height as u32;

                        if has_alpha && channels == 4 {
                            if let Some(img) = RgbaImage::from_raw(width, height, data) {
                                img.save(&filename)?;
                                println!(" -> Successfully saved image to: {}", filename);
                            }
                        } else if !has_alpha && channels == 3 {
                            if let Some(img) = RgbImage::from_raw(width, height, data) {
                                img.save(&filename)?;
                                println!(" -> Successfully saved image to: {}", filename);
                            }
                        } else {
                            println!(
                                " -> Unsupported image format: alpha={}, channels={}",
                                has_alpha, channels
                            );
                        }
                    } else {
                        println!(" -> Failed to deserialize image structure from D-Bus payload.");
                    }
                } else if let Some(path) = hints.get("image-path") {
                    // Occasionally apps might just pass a local file path as a string
                    println!(" -> Found an image path instead: {:?}", path);
                } else {
                    println!(" -> No image data attached to this notification.");
                }
            }
        }
    }

    Ok(())
}
