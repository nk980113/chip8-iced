use std::collections::LinkedList;

use iced::futures::FutureExt;
use iced::theme::Button;
use iced::time::{every, Duration};
use iced::widget::canvas::Cache;
use iced::{Application, Border, Color, Command, Element, Length, Point, Settings, Size, Subscription};
use iced::widget::{
    canvas::Program,
    container,
    container::Appearance,
    text,
    Canvas,
    column,
    button,
    row,
    scrollable,
    scrollable::{Alignment, Properties},
    Container,
};

fn main() -> Result<(), iced::Error> {
    App::run(Settings {
        window: iced::window::Settings {
            size: Size { width: 1024., height: 640. },
            resizable: false,
            ..Default::default()
        },
        ..Default::default()
    })
}

#[derive(Default)]
struct App {
    emulator: Option<Emulator>,
    logs: Vec<String>,
}

#[derive(Debug, Clone)]
enum Message {
    LoadROM,
    ROMOpened(Option<(Vec<u8>, String)>),
    ClearLog,
    Tick,
}

impl Application for App {
    type Message = Message;
    type Executor = iced::executor::Default;
    type Theme = iced::Theme;
    type Flags = ();

    fn new(_flags: ()) -> (Self, Command<Message>) {
        (Default::default(), Command::none())
    }

    fn title(&self) -> String {
        "Chip-8 Emulator".into()
    }

    fn update(&mut self, message: Self::Message) -> Command<Message> {
        match message {
            Message::LoadROM => {
                let dialog = rfd::AsyncFileDialog::new()
                    .set_title("Load ROM...")
                    .add_filter("Chip-8 ROM", &["ch8"])
                    .pick_file()
                    .then(|opt| async { match opt {
                        Some(handle) => Some((handle.read().await, handle.path().to_string_lossy().into())),
                        None => None,
                    } });
                return Command::perform(dialog, Message::ROMOpened);
            },
            Message::ROMOpened(None) => self.logs.push("Dialog closed".to_owned()),
            Message::ROMOpened(Some((rom, filename))) => {
                let rom_len = rom.len();
                if let Some(emulator) = Emulator::new(rom) {
                    self.emulator = Some(emulator);
                    self.logs.clear();
                    self.logs.push(format!("ROM loaded: {filename}"));
                    if rom_len & 1 > 0 {
                        self.logs.push(format!("Warning: ROM size {rom_len} is odd. This may cause undefined behaviors."))
                    }
                } else {
                    self.logs.push(format!("Error when loading {filename}: ROM size too big"));
                }
            },
            Message::ClearLog => self.logs.clear(),
            Message::Tick => {
                self.emulator.as_mut().unwrap().tick(&mut self.logs)
            },
        }
        Command::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let top_bar = container(
            row![
                button("Load ROM...").on_press(Message::LoadROM),
                button("Clear Logs").on_press_maybe(if self.logs.len() > 0 { Some(Message::ClearLog) } else { None } ).style(Button::Secondary),
            ].spacing(10)
        ).center_x().center_y().height(Length::Fill).width(Length::Fill);

        let middle_box = if let Some(emulator) = &self.emulator {
            container(Canvas::new(emulator).width(Length::Fill).height(Length::Fill))
        } else {
            container(text("Load ROM to see the effects!")).style(|_theme: &_| Appearance {
                border: Border {
                    color: Color::from_rgb(1., 0., 0.),
                    width: 5.,
                    ..Default::default()
                },
                ..Default::default()
            }).center_x().center_y()
        }.height(512.).width(Length::Fill);

        let logs: Container<'_, Message> = if self.logs.len() > 0 {
            container(
                scrollable(column(self.logs.iter().map(|log| text(log).into()))).width(Length::Fill)
                    .direction(scrollable::Direction::Vertical(Properties::new().alignment(Alignment::End)))
            )
        } else {
            container(text("Logs will appear here!").style(Color::from_rgb(0.5, 0.5, 0.5))).center_x().center_y()
        }.padding(5.).height(80.).width(Length::Fill);

        container(column![
            top_bar,
            middle_box,
            logs,
        ]).into()
    }

    fn subscription(&self) -> Subscription<Message> {
        if self.emulator.is_some() {
            every(Duration::from_secs_f32(1. / 700.)).map(|_| Message::Tick)
        } else {
            Subscription::none()
        }
    }
}

struct Emulator {
    screen: Screen,
    memory: [u8; 4096],
    pc: u16,
    reg_i: u16,
    reg_v: [u8; 16],
    stack: LinkedList<u16>,
    // TODO: implement more fields
}

impl Emulator {
    fn new(rom: Vec<u8>) -> Option<Self> {
        let mut memory = [0; 4096];

        let Some(prog_mem) = memory.get_mut(0x200..(0x200 + rom.len())) else { return None; };

        prog_mem.copy_from_slice(&rom);

        let Some(font_mem) = memory.get_mut(0x50..0xA0) else { unreachable!(); };

        font_mem.copy_from_slice(&FONT);


        Some(Self {
            memory,
            screen: Screen::default(),
            pc: 0x200,
            reg_i: 0,
            reg_v: [0; 16],
            stack: LinkedList::new(),
        })
    }

    fn tick(&mut self, logs: &mut Vec<String>) {
        let [first, nn, ..] = &self.memory[(self.pc as usize)..] else { unreachable!(); };
        let (first, nn) = (*first, *nn);

        let opcat = (first & 0xF0) >> 4;
        let x = first & 0xF;
        let y = (nn & 0xF0) >> 4;
        let n = nn & 0xF;
        let nnn = ((x as u16) << 8) + nn as u16;

        self.pc += 2;

        match opcat {
            // 00E0
            0 if nnn == 0x0E0 => {
                self.screen.clear();
            },
            // 00EE
            0 if nnn == 0x0EE => {
                if let Some(pc) = self.stack.pop_back() {
                    self.pc = pc;
                } else {
                    logs.push("Warning: attempted to return while stack is empty.".into());
                }
            }
            // 1NNN
            1 => {
                self.pc = nnn;
            },
            // 2NNN
            2 => {
                self.stack.push_back(self.pc);
                self.pc = nnn;
            },
            // 3XNN
            3 => {
                if self.reg_v[x as usize] == nn {
                    self.pc += 2;
                }
            },
            // 4XNN
            4 => {
                if self.reg_v[x as usize] != nn {
                    self.pc += 2;
                }
            },
            // 5XY0
            5 if n == 0 => {
                if self.reg_v[x as usize] == self.reg_v[y as usize] {
                    self.pc += 2;
                }
            },
            // 6XNN
            6 => {
                self.reg_v[x as usize] = nn;
            },
            // 7XNN
            7 => {
                self.reg_v[x as usize] = self.reg_v[x as usize].wrapping_add(nn);
            },
            // 8XY0
            8 if n == 0 => {
                self.reg_v[x as usize] = self.reg_v[y as usize];
            },
            // 8XY1
            8 if n == 1 => {
                self.reg_v[x as usize] |= self.reg_v[y as usize];
            },
            // 8XY2
            8 if n == 2 => {
                self.reg_v[x as usize] &= self.reg_v[y as usize];
            },
            // 8XY3
            8 if n == 3 => {
                self.reg_v[x as usize] ^= self.reg_v[y as usize];
            },
            // 8XY4
            8 if n == 4 => {
                let (x_new, carry) = self.reg_v[x as usize].overflowing_add(self.reg_v[y as usize]);
                self.reg_v[x as usize] = x_new;
                self.reg_v[0xF] = carry.into();
            },
            // 8XY5
            8 if n == 5 => {
                let (x_new, carry) = self.reg_v[x as usize].overflowing_sub(self.reg_v[y as usize]);
                self.reg_v[x as usize] = x_new;
                self.reg_v[0xF] = (!carry).into();
            },
            // 8XY6
            8 if n == 6 => {
                self.reg_v[x as usize] = self.reg_v[y as usize];
                let vf = self.reg_v[x as usize] & 1;
                self.reg_v[x as usize] = self.reg_v[x as usize] >> 1;
                self.reg_v[0xF] = vf;
            },
            // 8XY7
            8 if n == 7 => {
                let (x_new, carry) = self.reg_v[y as usize].overflowing_sub(self.reg_v[x as usize]);
                self.reg_v[x as usize] = x_new;
                self.reg_v[0xF] = (!carry).into();
            },
            // 8XYE
            8 if n == 0xE => {
                self.reg_v[x as usize] = self.reg_v[y as usize];
                let vf = (self.reg_v[x as usize] > 0x80).into();
                self.reg_v[x as usize] = self.reg_v[x as usize] << 1;
                self.reg_v[0xF] = vf;
            },
            // 9XY0
            9 if n == 0 => {
                if self.reg_v[x as usize] != self.reg_v[y as usize] {
                    self.pc += 2;
                }
            },
            // ANNN
            0xA => {
                self.reg_i = nnn;
            },
            // DXYN
            0xD => {
                let i_usize = self.reg_i as usize;
                self.reg_v[0xF] = self.screen.draw_sprite(
                    &self.memory[i_usize..(i_usize + n as usize)],
                    self.reg_v[x as usize] & 63,
                    self.reg_v[y as usize] & 31,
                ).into();
            },
            // FX33
            0xF if nn == 0x33 => {
                let i_usize = self.reg_i as usize;
                let units = self.reg_v[x as usize] % 10;
                let rest = self.reg_v[x as usize] / 10;
                let tens = rest % 10;
                let hundreds = rest / 10;
                self.memory[i_usize] = hundreds;
                self.memory[i_usize + 1] = tens;
                self.memory[i_usize + 2] = units;
            },
            // FX55
            0xF if nn == 0x55 => {
                let i_usize = self.reg_i as usize;
                self.memory[i_usize..=(i_usize + x as usize)].copy_from_slice(&self.reg_v[0..=x as usize]);
            },
            // FX65
            0xF if nn == 0x65 => {
                let i_usize = self.reg_i as usize;
                self.reg_v[0..=x as usize].copy_from_slice(&self.memory[i_usize..=(i_usize + x as usize)]);
            },
            _ => {
                logs.push(format!("Unknown instruction {first:x?}{nn:x?}; skipping"));
            }
        }
    }
}

#[derive(Default)]
struct Screen {
    content: [u64; 32],
    cache: Cache,
}

impl Screen {
    fn clear(&mut self) {
        self.cache.clear();
        self.content = [0; 32];
    }

    fn draw_sprite(&mut self, sprite: &[u8], x: u8, y: u8) -> bool {
        self.cache.clear();
        let mut vf_ret = false;
        let bitshift_count: i8 = 56 - x as i8;
        match bitshift_count {
            x if x > 0 => for row in 0..(sprite.len().min(32 - y as usize)) {
                let shifted: u64 = (sprite[row] as u64) << x;
                let target = &mut self.content[y as usize + row];
                if *target & shifted > 0 { vf_ret = true; }
                *target = *target ^ shifted;
            },
            x if x < 0 => for row in 0..(sprite.len().min(32 - y as usize)) {
                let shifted: u64 = sprite[row] as u64 >> x;
                let target = &mut self.content[y as usize + row];
                if *target & shifted > 0 { vf_ret = true; }
                *target = *target ^ shifted;
            },
            _ => for row in 0..(sprite.len().min(32 - y as usize)) {
                let draw_target: u64 = sprite[row].into();
                let target = &mut self.content[y as usize + row];
                if *target & draw_target > 0 { vf_ret = true; }
                *target = *target ^ draw_target;
            },
        }
        vf_ret
    }
}

const PIXEL_SIZE: f32 = 16.;

impl Program<Message> for Emulator {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        let pixels = self.screen.cache.draw(renderer, bounds.size(), |frame| {
            for (i, row) in self.screen.content.iter().enumerate() {
                let y = (i as f32) * PIXEL_SIZE;
                let mut row = *row;

                for x in (0..64).rev().map(|x| (x as f32) * PIXEL_SIZE) {
                    frame.fill_rectangle(
                        Point { x, y },
                        Size { width: PIXEL_SIZE, height: PIXEL_SIZE },
                        if row & 1 > 0 { Color::WHITE } else { Color::BLACK },
                    );

                    row = row >> 1;
                }
            }
        });
        vec![pixels]
    }
}

// from https://tobiasvl.github.io/blog/write-a-chip-8-emulator/#font
const FONT: [u8; 80] = [
    0xF0, 0x90, 0x90, 0x90, 0xF0, // 0
    0x20, 0x60, 0x20, 0x20, 0x70, // 1
    0xF0, 0x10, 0xF0, 0x80, 0xF0, // 2
    0xF0, 0x10, 0xF0, 0x10, 0xF0, // 3
    0x90, 0x90, 0xF0, 0x10, 0x10, // 4
    0xF0, 0x80, 0xF0, 0x10, 0xF0, // 5
    0xF0, 0x80, 0xF0, 0x90, 0xF0, // 6
    0xF0, 0x10, 0x20, 0x40, 0x40, // 7
    0xF0, 0x90, 0xF0, 0x90, 0xF0, // 8
    0xF0, 0x90, 0xF0, 0x10, 0xF0, // 9
    0xF0, 0x90, 0xF0, 0x90, 0x90, // A
    0xE0, 0x90, 0xE0, 0x90, 0xE0, // B
    0xF0, 0x80, 0x80, 0x80, 0xF0, // C
    0xE0, 0x90, 0x90, 0x90, 0xE0, // D
    0xF0, 0x80, 0xF0, 0x80, 0xF0, // E
    0xF0, 0x80, 0xF0, 0x80, 0x80  // F
];