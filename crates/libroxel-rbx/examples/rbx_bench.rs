//! Times opening a big place: saves one with many parts (as Studio would)
//! in both formats, then reads each and builds its tree.
//!
//! cargo run -p libroxel-rbx --example rbx_bench -- [parts]
use std::time::Instant;

use rbx_dom_weak::types::{Attributes, CFrame, Color3uint8, Enum, Matrix3, Tags, Variant, Vector3};
use rbx_dom_weak::{InstanceBuilder, WeakDom};

fn main() {
    let parts: usize = std::env::args().nth(1).and_then(|n| n.parse().ok()).unwrap_or(100_000);
    let mut workspace = InstanceBuilder::new("Workspace");
    for m in 0..parts / 10 {
        let mut model = InstanceBuilder::new("Model").with_name(format!("House{m}"));
        let mut tags = Tags::new();
        tags.push("House");
        let mut attributes = Attributes::new();
        attributes.insert("Owner".into(), Variant::String("nobody".into()));
        model = model.with_property("Tags", tags).with_property("Attributes", attributes);
        for p in 0..10 {
            let x = (m * 10 + p) as f32;
            model = model.with_child(
                InstanceBuilder::new(if p % 3 == 0 { "MeshPart" } else { "Part" })
                    .with_name(format!("Wall{p}"))
                    .with_property("CFrame", CFrame::new(Vector3::new(x, 2.0, 0.0), Matrix3::identity()))
                    .with_property("Size", Vector3::new(4.0, 8.0, 1.0))
                    .with_property("Anchored", true)
                    .with_property("Color", Color3uint8::new(200, 180, 160))
                    .with_property("Material", Enum::from_u32(512))
                    .with_property("TopSurface", Enum::from_u32(0)),
            );
        }
        workspace = workspace.with_child(model);
    }
    let dom = WeakDom::new(InstanceBuilder::new("DataModel").with_child(workspace));
    let only_binary = std::env::args().any(|a| a == "binary");
    for binary in [true, false].into_iter().filter(|b| *b || !only_binary) {
        let mut bytes = Vec::new();
        let roots = dom.root().children().to_vec();
        if binary {
            rbx_binary::to_writer(&mut bytes, &dom, &roots).unwrap();
        } else {
            rbx_xml::to_writer_default(&mut bytes, &dom, &roots).unwrap();
        }
        let t = Instant::now();
        let read = libroxel_rbx::read_dom(&bytes).unwrap();
        let read_time = t.elapsed();
        let t = Instant::now();
        let mut tree = libroxel_instance::InstanceTree::new();
        let report = libroxel_rbx::import_place(&mut tree, &read);
        let build_time = t.elapsed();
        println!(
            "{}: {} KB, {} instances: read {:?}, build {:?}",
            if binary { "binary" } else { "xml" },
            bytes.len() / 1024,
            report.instances,
            read_time,
            build_time
        );
    }
}
