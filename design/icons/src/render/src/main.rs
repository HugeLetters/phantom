// usage: render <in.svg> <out.png> <width> [height]
use resvg::{tiny_skia, usvg};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let input = std::path::Path::new(&a[1]);
    let w: u32 = a[3].parse().unwrap();
    let data = std::fs::read(input).unwrap();
    let mut opt = usvg::Options::default();
    opt.resources_dir = input.parent().map(|p| p.to_path_buf());
    opt.fontdb_mut().load_system_fonts();
    let tree = usvg::Tree::from_data(&data, &opt).unwrap();
    let size = tree.size();
    let h: u32 = a.get(4).map(|s| s.parse().unwrap()).unwrap_or((w as f32 * size.height() / size.width()).round() as u32);
    let mut pm = tiny_skia::Pixmap::new(w, h).unwrap();
    let t = tiny_skia::Transform::from_scale(w as f32 / size.width(), h as f32 / size.height());
    resvg::render(&tree, t, &mut pm.as_mut());
    pm.save_png(&a[2]).unwrap();
}
