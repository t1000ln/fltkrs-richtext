use fltk::{app, window};
use fltk::enums::Font;
use fltk::prelude::{GroupExt, WidgetExt, WindowExt};
use fltkrs_richdisplay::rich_text::RichText;
use fltkrs_richdisplay::UserData;

fn main() {
    simple_logger::init_with_level(log::Level::Debug).unwrap();


    let app = app::App::default().load_system_fonts();
    let mut win = window::Window::default()
        .with_size(1220, 820)
        .with_label("rich-display newline test")
        .center_screen();
    win.make_resizable(true);

    let mut rich_text = RichText::new(10, 10, 1200, 800, None);
    rich_text.set_text_font(Font::by_index(13));
    rich_text.set_text_size(22);

    win.end();
    win.show();

    let data = vec![
        UserData::new_text("Rust is a multi-paradigm, general-purpose programming language that emphasizes performance, type safety, and concurrency.It enforces memory safety, meaning that all references point to valid memory... \r\n".to_string()),
        UserData::new_text("Desktop\r\n".to_string()),
        UserData::new_text("\r\n".to_string()),
        UserData::new_text("dev@my_host:~$ \r\n".to_string()),
        UserData::new_text("Software developer Graydon Hoare created Rust as a personal project while working at Mozilla Research in 2006.".to_string())
    ];

    for ud in data {
        rich_text.append(ud);
    }

    app.run().unwrap();
}