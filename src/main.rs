// Terminal Tetris for PS3 Linux (ppc64), no external crates.
// Build: cargo build --release && ./target/release/tetris
// Keys: arrows or a/d=move, w/space=rotate, s=soft drop, c=hold, p=pause, q=quit.
use std::io::Write;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const W: usize = 10;
const H: usize = 20;

// Each piece: rotations, each a set of (x,y) cells.
const PIECES: [&[[(i8, i8); 4]]; 7] = [
    &[[(0,0),(1,0),(2,0),(3,0)], [(2,0),(2,1),(2,2),(2,3)], [(0,2),(1,2),(2,2),(3,2)], [(0,0),(0,1),(0,2),(0,3)]], // I
    &[[(0,0),(1,0),(0,1),(1,1)]], // O
    &[[(1,0),(0,1),(1,1),(2,1)], [(1,0),(1,1),(1,2),(2,1)], [(0,1),(1,1),(2,1),(1,2)], [(1,0),(0,1),(1,1),(1,2)]], // T
    &[[(0,0),(0,1),(1,1),(2,1)], [(1,0),(2,0),(1,1),(1,2)], [(0,1),(1,1),(2,1),(2,2)], [(1,0),(1,1),(0,2),(1,2)]], // J
    &[[(2,0),(0,1),(1,1),(2,1)], [(1,0),(1,1),(1,2),(2,2)], [(0,1),(1,1),(2,1),(0,2)], [(0,0),(1,0),(1,1),(1,2)]], // L
    &[[(1,0),(2,0),(0,1),(1,1)], [(1,0),(1,1),(2,1),(2,2)]], // S
    &[[(0,0),(1,0),(1,1),(2,1)], [(2,0),(1,1),(2,1),(1,2)]], // Z
];
const COLORS: [u8; 7] = [96, 93, 95, 94, 33, 92, 91]; // ansi fg per piece

extern "C" {
    fn tcgetattr(fd: i32, termios: *mut u8) -> i32;
    fn tcsetattr(fd: i32, act: i32, termios: *const u8) -> i32;
    fn poll(fds: *mut PollFd, nfds: u64, timeout: i32) -> i32;
    fn read(fd: i32, buf: *mut u8, count: usize) -> isize;
}
#[repr(C)]
struct PollFd { fd: i32, events: i16, revents: i16 }

// PowerPC Linux termios: iflag/oflag/cflag/lflag u32 @0/4/8/12, cc[] @17.
// VMIN=5, VTIME=7, ISIG=0x80, ICANON=0x100, ECHO=0x8 (ppc values, NOT x86!).
const LFLAG_OFF: usize = 12;
const CC_OFF: usize = 17;

struct TermRaw { orig: [u8; 64] }
impl TermRaw {
    fn new() -> TermRaw {
        let mut t = [0u8; 64];
        unsafe {
            tcgetattr(0, t.as_mut_ptr());
            let orig = t;
            let lflag = u32::from_ne_bytes(t[LFLAG_OFF..LFLAG_OFF + 4].try_into().unwrap())
                & !(0x80 | 0x100 | 0x8); // clear ISIG, ICANON, ECHO (ppc values)
            t[LFLAG_OFF..LFLAG_OFF + 4].copy_from_slice(&lflag.to_ne_bytes());
            t[CC_OFF + 5] = 1; // VMIN
            t[CC_OFF + 7] = 0; // VTIME
            tcsetattr(0, 0, t.as_ptr());
            TermRaw { orig }
        }
    }
}
impl Drop for TermRaw {
    fn drop(&mut self) { unsafe { tcsetattr(0, 0, self.orig.as_ptr()); } }
}

// Input byte queue: must bypass std::io::stdin(), whose buffer swallows the
// trailing bytes of arrow-key escape sequences that poll() can no longer see.
fn wait_input(q: &mut Vec<u8>, ms: i32) -> Option<u8> {
    if q.is_empty() {
        let mut fds = [PollFd { fd: 0, events: 1, revents: 0 }];
        if unsafe { poll(fds.as_mut_ptr(), 1, ms) } <= 0 { return None; }
        let mut b = [0u8; 64];
        let n = unsafe { read(0, b.as_mut_ptr(), b.len()) };
        if n <= 0 { return None; }
        q.extend_from_slice(&b[..n as usize]);
    }
    Some(q.remove(0))
}

#[derive(PartialEq)]
enum Key { Left, Right, Down, Up, Quit, Pause, Hold, Other }

fn read_key(q: &mut Vec<u8>, ms: i32) -> Option<Key> {
    let b = wait_input(q, ms)?;
    let k = match b {
        b'\x1b' => {
            // Arrow arrives as a 3-byte burst (ESC [ X); collect and decode.
            let s: Vec<u8> = (0..2).filter_map(|_| wait_input(q, 20)).collect();
            match s.as_slice() {
                [b'[', x] | [b'O', x] => match x {
                    b'A' => Key::Up, b'B' => Key::Down, b'C' => Key::Right, b'D' => Key::Left,
                    _ => Key::Other,
                },
                _ => Key::Other,
            }
        }
        b'a' | b'h' | b'A' | b'H' => Key::Left,
        b'd' | b'l' | b'D' | b'L' => Key::Right,
        b's' | b'S' | b'j' | b'J' => Key::Down,
        b'w' | b'W' | b' ' | b'k' | b'K' => Key::Up,
        b'q' | b'Q' | 3 => Key::Quit,
        b'p' | b'P' => Key::Pause,
        b'c' | b'C' => Key::Hold,
        _ => Key::Other,
    };
    Some(k)
}

#[derive(Clone)]
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
    board: [[u8; W]; H], cur: Piece, next: usize,
    hold: Option<usize>, hold_used: bool,
    score: u32, lines: u32, level: u32, over: bool, bag: Vec<usize>,
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
    // 7-bag randomizer, seeded by the clock
    fn draw(&mut self) -> usize {
        if self.bag.is_empty() {
            self.bag = (0..7).collect();
            for i in (1..7).rev() {
                let j = (SystemTime::now().duration_since(UNIX_EPOCH).unwrap().subsec_nanos()
                    % (i as u32 + 1)) as usize;
                self.bag.swap(i, j);
            }
        }
        self.bag.pop().unwrap()
    }
    fn fits(&self, p: &Piece) -> bool {
        p.cells().iter().all(|&(x, y)| {
            x >= 0 && x < W as i8 && y < H as i8
                && (y < 0 || self.board[y as usize][x as usize] == 0)
        })
    }
    fn move_(&mut self, dx: i8, dy: i8) -> bool {
        let mut p = self.cur.clone();
        p.x += dx; p.y += dy;
        if self.fits(&p) { self.cur = p; true } else { false }
    }
    fn rotate(&mut self) {
        let mut p = self.cur.clone();
        p.rot = (p.rot + 1) % PIECES[p.kind].len();
        for k in [0i8, -1, 1, -2, 2] { // simple wall kicks
            p.x += k;
            if self.fits(&p) { self.cur = p; return; }
            p.x -= k;
        }
    }
    fn lock(&mut self) {
        for &(x, y) in self.cur.cells().iter() {
            if y >= 0 { self.board[y as usize][x as usize] = (self.cur.kind + 1) as u8; }
        }
        let mut cleared = 0;
        let mut y = H;
        while y > 0 {
            y -= 1;
            if self.board[y].iter().all(|&c| c != 0) {
                cleared += 1;
                self.board[..=y].rotate_right(1);
                self.board[0] = [0; W];
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
        self.over = self.cur.cells().iter().any(|&(_, y)| y < 0) || !self.fits(&self.cur);
    }
    fn hold(&mut self) {
        if self.hold_used { return; }
        self.hold_used = true;
        let k = self.cur.kind;
        self.cur = match self.hold.take() {
            Some(h) => Piece::new(h),
            None => { let n = self.next; self.next = self.draw(); Piece::new(n) }
        };
        self.hold = Some(k);
    }
    fn piece_at(&self, x: usize, y: usize) -> u8 {
        let k = (self.cur.kind + 1) as u8;
        if self.cur.cells().iter().any(|&(cx, cy)| cx as usize == x && cy as usize == y) { k }
        else { self.board[y][x] }
    }
    fn render(&self, paused: bool) -> String {
        let title = if paused { "  PA3A (pause)  " } else { "  TETRIS  " };
        let mut out = format!("\x1b[H\x1b[1;97m=====\x1b[0m{} \x1b[1;97m=====\x1b[0m\r\n\r\n\x1b[90m+------------+  \x1b[0m\r\n", title);
        for y in 0..H {
            out.push_str("\x1b[90m|\x1b[0m");
            for x in 0..W {
                let k = self.piece_at(x, y);
                if k == 0 { out.push_str("  "); }
                else { out.push_str(&format!("\x1b[{}m[]\x1b[0m", COLORS[(k - 1) as usize])); }
            }
            out.push_str("\x1b[90m|\x1b[0m");
            match y { // side info
                1 => out.push_str("  Next:"),
                2..=4 => for dx in 0..5i8 {
                    let dy = y as i8 - 2;
                    if PIECES[self.next][0].iter().any(|&(px, py)| px == dx && py == dy) {
                        out.push_str(&format!("\x1b[{}m[]\x1b[0m", COLORS[self.next]));
                    } else { out.push_str("  "); }
                },
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
            out.push_str("\x1b[K\r\n");
        }
        out.push_str("\x1b[90m+------------+\x1b[0m\r\n\x1b[K");
        if self.over {
            out.push_str(&format!("\r\n\x1b[1;91m GAME OVER!\x1b[0m Score: {}. Enter = restart, Q = quit\r\n", self.score));
        }
        out
    }
}

fn main() {
    let _raw = TermRaw::new();
    let mut out = std::io::stdout();
    let mut g = Game::new();
    let mut paused = false;
    let mut last = Instant::now();
    let mut inq = Vec::new();

    write!(out, "\x1b[?25l").unwrap();
    out.flush().unwrap();

    loop {
        if g.over {
            write!(out, "{}", g.render(false)).unwrap();
            out.flush().unwrap();
            match read_key(&mut inq, 200) {
                Some(Key::Quit) => break,
                Some(Key::Other) => g = Game::new(),
                _ => {}
            }
            continue;
        }
        let interval = Duration::from_millis((800u64.saturating_sub((g.level as u64 - 1) * 70)).max(80));
        let remain = interval.saturating_sub(last.elapsed()).as_millis().min(100) as i32;
        match read_key(&mut inq, remain) {
            Some(Key::Quit) => break,
            Some(Key::Pause) => paused = !paused,
            Some(k) if !paused => match k {
                Key::Left => { g.move_(-1, 0); },
                Key::Right => { g.move_(1, 0); },
                Key::Down => { if !g.move_(0, 1) { g.lock(); } g.score += 1; },
                Key::Up => g.rotate(),
                Key::Hold => g.hold(),
                _ => {}
            },
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
