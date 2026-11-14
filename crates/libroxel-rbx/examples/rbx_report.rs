//! Prints what importing each file named on the command line brings across.
fn main() {
    for path in std::env::args().skip(1) {
        println!("== {path}");
        match libroxel_rbx::open(std::path::Path::new(&path)) {
            Ok((_, report)) => {
                for line in report.summary() {
                    println!("  {line}");
                }
            }
            Err(e) => println!("  error: {e}"),
        }
    }
}
