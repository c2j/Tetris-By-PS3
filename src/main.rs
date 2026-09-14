// Terminal Tetris for PS3 Linux (ppc64), no external crates.
// Build: cargo build --release && ./target/release/tetris
// Keys: a/d or h/l = move, w or space = rotate, s = soft drop,
//       q = quit, p = pause, c = hold. Enter to start/restart.
use std::io::{Read, Write};
use std::time::{Duration, Instant};

const W: usize = 10;
const H: usize = 20;

// Each piece: list of rotations, each rotation a set of (x,y) cells.
const PIECES: [&[[(i8, i8); 4]]; 7] = [
    &[[(0,0),(1,0),(2,0),(3,0)], [(2,0),(2,1),(2,2),(2,3)], [(0,2),(1,2),(2,2),(3,2)], [(0,0),(0,1),(0,2),(0,3)]], // I
    &[[(0,0),(1,0),(0,1),(1,1)]], // O
    &[[(1,0),(0,1),(1,1),(2,1)], [(1,0),(1,1),(1,2),(2,1)], [(0,1),(1,1),(2,1),(1,2)], [(1,0),(0,1),(1,1),(1,2)]], // T
    &[[(0,0),(0,1),(1,1),(2,1)], [(1,0),(2,0),(1,1),(1,2)], [(0,1),(1,1),(2,1),(2,2)], [(1,0),(1,1),(0,2),(1,2)]], // J
    &[[(2,0),(0,1),(1,1),(2,1)], [(1,0),(1,1),(1,2),(2,2)], [(0,1),(1,1),(2,1),(0,2)], [(0,0),(1,0),(1,1),(1,2)]], // L
    &[[(1,0),(2,0),(0,1),(1,1)], [(1,0),(1,1),(2,1),(2,2)]], // S
    &[[(0,0),(1,0),(1,1),(2,1)], [(2,0),(1,1),(2,1),(1,2)]], // Z
];

const COLORS: [u8; 7] = [96, 93, 95, 94, 33, 92, 91]; // ansi fg colors per piece

struct TermRaw {
    orig: Vec<u8>,
}

extern "C" {
    fn tcgetattr(fd: i32, termios: *mut u8) -> i32;
    fn tcsetattr(fd: i32, act: i32, termios: *const u8) -> i32;
    fn poll(fds: *mut PollFd, nfds: u64, timeout: i32) -> i32;
}

#[repr(C)]
struct PollFd { fd: i32, events: i16, revents: i16 }

// Linux termios: iflag, oflag, cflag, lflag (u32 each), line (u8), cc[32],
// plus c_ispeed/c_ospeed (u32 each) = 57 bytes, padded to 60. Use 64 to be safe.
const TERMIOS_LEN: usize = 64;
const LFLAG_OFF: usize = 12;
const CC_OFF: usize = 17;
const VTIME: usize = 7;
const VMIN: usize = 5;
const ISIG: u32 = 0x80;
const ICANON: u32 = 0x100;
const ECHO: u32 = 0x8;
const TCSANOW: i32 = 0;

fn get_word(t: &[u8], off: usize) -> u32 {
    u32::from_ne_bytes([t[off], t[off + 1], t[off + 2], t[off + 3]])
}

impl TermRaw {
    fn new() -> TermRaw {
        let mut t = vec![0u8; TERMIOS_LEN];
        unsafe {
            if tcgetattr(0, t.as_mut_ptr()) != 0 {
                eprintln!("tcgetattr failed");
            }
            let orig = t.clone();
            let lflag = get_word(&t, LFLAG_OFF) & !(ISIG | ICANON | ECHO);
            t[LFLAG_OFF..LFLAG_OFF + 4].copy_from_slice(&lflag.to_ne_bytes());
            t[CC_OFF + VMIN] = 1;
            t[CC_OFF + VTIME] = 0;
            tcsetattr(0, TCSANOW, t.as_ptr());
            TermRaw { orig }
        }
    }
}

impl Drop for TermRaw {
    fn drop(&mut self) {
        unsafe { tcsetattr(0, TCSANOW, self.orig.as_ptr()); }
    }
}

fn wait_input(ms: i32) -> Option<u8> {
    let mut fds = [PollFd { fd: 0, events: 1, revents: 0 }];
    let r = unsafe { poll(fds.as_mut_ptr(), 1, ms) };
    if r <= 0 { return None; }
    let mut b = [0u8; 1];
    if std::io::stdin().read(&mut b).unwrap() == 0 { return None; }
    Some(b[0])
}

// returns Some(extended key) for escape sequences like arrows
fn read_key(ms: i32) -> Option<Key> {
    let b = wait_input(ms)?;
    match b {
        b'\x1b' => {
            // try read sequence
            match wait_input(0) {
                Some(b'[') => match wait_input(0) {
                    Some(b'A') => Some(Key::Up),
                    Some(b'B') => Some(Key::Down),
                    Some(b'C') => Some(Key::Right),
                    Some(b'D') => Some(Key::Left),
                    _ => Some(Key::Other),
                },
                _ => Some(Key::Other),
            }
        }
        b'a' | b'h' | b'A' | b'H' => Some(Key::Left),
        b'd' | b'l' | b'D' | b'L' => Some(Key::Right),
        b's' | b'S' | b'j' | b'J' => Some(Key::Down),
        b'w' | b'W' | b' ' | b'k' | b'K' => Some(Key::Up),
        b'q' | b'Q' | 3 | 27 => Some(Key::Quit),
        b'p' | b'P' => Some(Key::Pause),
        b'c' | b'C' => Some(Key::Hold),
        _ => Some(Key::Other),
    }
}

#[derive(PartialEq)]
enum Key { Left, Right, Down, Up, Quit, Pause, Hold, Other }

struct Piece { kind: usize, rot: usize, x: i8, y: i8 }

impl Piece {
    fn new(kind: usize) -> Piece { Piece { kind, rot: 0, x: 3, y: 0 } }
    fn cells(&self) -> [(i8, i8); 4] {
        let mut c = PIECES[self.kind][self.rot];
        for p in c.iter_mut() { p.0 += self.x; p.1 += self.y; }
        c
    }
}

struct Game {
    board: [[u8; W]; H],
    cur: Piece,
    next: usize,
    hold: Option<usize>,
    hold_used: bool,
    score: u32,
    lines: u32,
    level: u32,
    over: bool,
    bag: Vec<usize>,
}

impl Game {
    fn new() -> Game {
        let mut g = Game {
            board: [[0; W]; H], cur: Piece::new(0), next: 0, hold: None, hold_used: false,
            score: 0, lines: 0, level: 1, over: false, bag: Vec::new(),
        };
        g.cur = Piece::new(g.draw());
        g.next = g.draw();
        g
    }
    fn draw(&mut self) -> usize {
        if self.bag.is_empty() {
            self.bag = (0..7).collect();
            // shuffle
            for i in (1..7).rev() {
                let j = (std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos()
                    % (i as u32 + 1)) as usize;
                self.bag.swap(i, j);
            }
        }
        self.bag.pop().unwrap()
    }
    fn fits(&self, p: &Piece) -> bool {
        for &(x, y) in p.cells().iter() {
            if x < 0 || x >= W as i8 || y >= H as i8 { return false; }
            if y >= 0 && self.board[y as usize][x as usize] != 0 { return false; }
        }
        true
    }
    fn move_(&mut self, dx: i8, dy: i8) -> bool {
        let mut p = self.cur.clone_piece();
        p.x += dx; p.y += dy;
        if self.fits(&p) { self.cur = p; true } else { false }
    }
    fn rotate(&mut self) {
        let mut p = self.cur.clone_piece();
        p.rot = (p.rot + 1) % PIECES[p.kind].len();
        // simple wall kick
        for k in [0i8, -1, 1, -2, 2] {
            p.x += k;
            if self.fits(&p) { self.cur = p; return; }
            p.x -= k;
        }
    }
    fn lock(&mut self) {
        let mut top_out = false;
        for &(x, y) in self.cur.cells().iter() {
            if y < 0 { top_out = true; continue; }
            self.board[y as usize][x as usize] = (self.cur.kind + 1) as u8;
        }
        if top_out { self.over = true; }
        // clear lines
        let mut cleared = 0;
        let mut y = H;
        while y > 0 {
            y -= 1;
            if self.board[y].iter().all(|&c| c != 0) {
                cleared += 1;
                for yy in (1..=y).rev() {
                    self.board[yy] = self.board[yy - 1];
                }
                self.board[0] = [0; W];
                y += 1;
            }
        }
        if cleared > 0 {
            self.lines += cleared;
            self.score += [0u32, 100, 300, 500, 800][cleared as usize] * self.level;
            self.level = 1 + self.lines / 10;
        }
        self.cur = Piece::new(self.next);
        self.next = self.draw();
        self.hold_used = false;
        if !self.fits(&self.cur) { self.over = true; }
    }
    fn hold(&mut self) {
        if self.hold_used { return; }
        self.hold_used = true;
        let cur_kind = self.cur.kind;
        match self.hold.take() {
            Some(k) => self.cur = Piece::new(k),
            None => {
                let n = self.next;
                self.cur = Piece::new(n);
                self.next = self.draw();
            }
        }
        self.hold = Some(cur_kind);
    }

    fn render(&self, paused: bool) -> String {
        let mut out = String::with_capacity(8192);
        out.push_str("\x1b[H\x1b[2J");
        let title = if paused { "  PA3A (pause)  " } else { "  TETRIS  " };
        out.push_str(&format!("\x1b[1;97m=====\x1b[0m{} \x1b[1;97m=====\x1b[0m\r\n\r\n", title));
        // board frame
        out.push_str("\x1b[90m+------------+  \x1b[0m\r\n");
        let cells = self.cur.cells();
        for y in 0..H {
            out.push_str("\x1b[90m|\x1b[0m");
            for x in 0..W {
                let mut k = self.board[y][x];
                if !paused {
                    if cells.iter().any(|&(cx, cy)| cy as usize == y && cx as usize == x) {
                        k = (self.cur.kind + 1) as u8;
                    }
                }
                if k == 0 {
                    out.push_str("  ");
                } else {
                    out.push_str(&format!("\x1b[{}m[]\x1b[0m", COLORS[(k - 1) as usize]));
                }
            }
            out.push_str("\x1b[90m|\x1b[0m");
            // side info on some rows
            match y {
                1 => out.push_str("  Next:"),
                2..=4 => {
                    let nk = self.next;
                    let dy = y as i8 - 2;
                    out.push_str("  ");
                    for dx in 0..5i8 {
                        if PIECES[nk][0].iter().any(|&(px, py)| px == dx && py == dy) {
                            out.push_str(&format!("\x1b[{}m[]\x1b[0m", COLORS[nk]));
                        } else { out.push_str("  "); }
                    }
                }
                6 => out.push_str(&format!("  Score: {}", self.score)),
                7 => out.push_str(&format!("  Lines: {}", self.lines)),
                8 => out.push_str(&format!("  Level: {}", self.level)),
                10 => if let Some(h) = self.hold {
                    out.push_str(&format!("  Hold: \x1b[{}m[]\x1b[0m", COLORS[h]));
                },
                15 => out.push_str("  <- -> move"),
                16 => out.push_str("  up rotate, down soft-drop"),
                17 => out.push_str("  C hold, P pause"),
                18 => out.push_str("  Q quit"),
                _ => {}
            }
            out.push_str("\r\n");
        }
        out.push_str("\x1b[90m+------------+\x1b[0m\r\n");
        if self.over {
            out.push_str(&format!("\r\n\x1b[1;91m GAME OVER!\x1b[0m Score: {}. Enter = restart, Q = quit\r\n", self.score));
        }
        out
    }
}

impl Piece {
    fn clone_piece(&self) -> Piece { Piece { kind: self.kind, rot: self.rot, x: self.x, y: self.y } }
}

fn main() {
    let _raw = TermRaw::new();
    let mut out = std::io::stdout();
    let mut g = Game::new();
    let mut paused = false;
    let mut last = Instant::now();

    write!(out, "\x1b[?25l").unwrap();
    out.flush().unwrap();

    loop {
        if g.over {
            write!(out, "{}", g.render(false)).unwrap();
            out.flush().unwrap();
            match read_key(200) {
                Some(Key::Quit) => break,
                Some(Key::Other) => { g = Game::new(); }
                _ => {}
            }
            continue;
        }
        let interval = Duration::from_millis((800u64.saturating_sub((g.level as u64 - 1) * 70).max(80)));
        // input handling
        let elapsed = last.elapsed();
        let remain = interval.saturating_sub(elapsed);
        match read_key(remain.as_millis().min(100) as i32) {
            Some(Key::Quit) => break,
            Some(Key::Pause) => paused = !paused,
            Some(Key::Left) => if !paused { g.move_(-1, 0); },
            Some(Key::Right) => if !paused { g.move_(1, 0); },
            Some(Key::Down) => if !paused { if !g.move_(0, 1) { g.lock(); } g.score += 1; },
            Some(Key::Up) => if !paused { g.rotate(); },
            Some(Key::Hold) => if !paused { g.hold(); },
            _ => {}
        }
        if paused {
            write!(out, "{}", g.render(true)).unwrap();
            out.flush().unwrap();
            continue;
        }
        if last.elapsed() >= interval {
            if !g.move_(0, 1) { g.lock(); }
            last = Instant::now();
        }
        write!(out, "{}", g.render(false)).unwrap();
        out.flush().unwrap();
    }
    write!(out, "\x1b[?25h\x1b[0m\x1b[2J\x1b[H").unwrap();
    out.flush().unwrap();
    println!("Thanks for playing! Score: {}", g.score);
}
