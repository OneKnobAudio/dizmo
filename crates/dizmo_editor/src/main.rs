mod model;
mod ui;

use ui::App;

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .theme(App::app_theme)
        .window_size((1100, 720))
        .run()
}
