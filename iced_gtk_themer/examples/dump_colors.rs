use iced_gtk_themer::parser::parse_gtk_theme;
use std::fs;

fn main() {
    let css = fs::read_to_string("/usr/share/themes/adw-gtk3-dark/gtk-3.0/gtk.css")
        .or_else(|_| fs::read_to_string(dirs::home_dir().unwrap().join(".themes/adw-gtk3-dark/gtk-3.0/gtk.css"))).unwrap();
    let colors = parse_gtk_theme(&css);
    
    println!("Found {} colors", colors.len());
    for (k, v) in &colors {
        if k.contains("bg") || k.contains("fg") || k.contains("text") || k.contains("base") || k.contains("btn") {
            println!("{}: {:?}", k, v);
        }
    }
}
