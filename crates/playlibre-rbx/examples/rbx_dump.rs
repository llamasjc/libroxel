//! Prints every instance and property in a file, for working out how it maps.
fn main() {
    let path = std::env::args().nth(1).expect("a file");
    let dom = playlibre_rbx::read_dom(&std::fs::read(path).unwrap()).unwrap();
    let mut stack = vec![(dom.root_ref(), 0)];
    while let Some((r, depth)) = stack.pop() {
        let inst = dom.get_by_ref(r).unwrap();
        println!("{}{} ({})", "  ".repeat(depth), inst.name, inst.class);
        let mut props: Vec<_> = inst.properties.iter().collect();
        props.sort_by_key(|(k, _)| k.as_str());
        for (k, v) in props {
            let text = format!("{v:?}");
            println!("{}  .{k} = {}", "  ".repeat(depth), &text[..text.len().min(120)]);
        }
        for c in inst.children().iter().rev() {
            stack.push((*c, depth + 1));
        }
    }
}
