//! Pictures scripts draw (EditableImage) reach the screen.

use playlibre_script::ScriptRuntime;

#[test]
fn an_image_label_shows_the_pixels_a_script_wrote() {
    let rt = ScriptRuntime::new_with_tree(playlibre_instance::InstanceTree::new()).unwrap();
    rt.run(
        "Map",
        r#"
        local img = game:GetService("AssetService"):CreateEditableImage({ Size = Vector2.new(2, 1) })
        local b = buffer.create(2 * 1 * 4)
        buffer.writeu32(b, 0, 0xff0000ff) -- red
        buffer.writeu32(b, 4, 0xffff0000) -- blue
        img:WritePixelsBuffer(Vector2.zero, Vector2.new(2, 1), b)
        local label = Instance.new("ImageLabel")
        label.ImageContent = Content.fromObject(img)
        print(label.Image)
        "#,
    )
    .unwrap();
    assert!(rt.step(0.1).is_empty());
    let name = rt.take_output().remove(0).text;
    let drawn = playlibre_script::drawn::image(&name).expect("the label names a drawn picture");
    assert_eq!(drawn.size, [2, 1]);
    assert_eq!(drawn.rgba, [255, 0, 0, 255, 0, 0, 255, 255]);
}

/// What a Heartbeat handler draws shows the same frame, along with the
/// labels it moves there (a frame late, a minimap flickered).
#[test]
fn pixels_drawn_in_heartbeat_show_that_frame() {
    let rt = ScriptRuntime::new_with_tree(playlibre_instance::InstanceTree::new()).unwrap();
    rt.run(
        "Map",
        r#"
        local img = game:GetService("AssetService"):CreateEditableImage({ Size = Vector2.new(1, 1) })
        local label = Instance.new("ImageLabel")
        label.ImageContent = Content.fromObject(img)
        print(label.Image)
        local frame = 0
        game:GetService("RunService").Heartbeat:Connect(function()
            frame += 1
            local b = buffer.create(4)
            buffer.writeu32(b, 0, 0xff000000 + frame)
            img:WritePixelsBuffer(Vector2.zero, Vector2.new(1, 1), b)
        end)
        "#,
    )
    .unwrap();
    let name = rt.take_output().remove(0).text;
    for frame in 1..=3u8 {
        assert!(rt.step(0.1).is_empty());
        let drawn = playlibre_script::drawn::image(&name).expect("drawn");
        assert_eq!(drawn.rgba[0], frame, "frame {frame}'s pixels show in frame {frame}");
    }
}
