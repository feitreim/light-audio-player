use std::{env, vec::Vec, thread, io, 
    time::Duration, path::{Path, PathBuf}, 
    sync::mpsc::{channel, Sender, Receiver},
    fs::{File, read_dir}, };
use rand::{seq::SliceRandom, thread_rng};
use rodio::{Decoder, OutputStream, Sink};

// ---------------------- Constants ---------------------
const VOLUME_ADJUSTMENT: f32 = 0.1f32;
const KEEP_ALIVE_TIME: u64 = 30;
struct Message {
    contents: Contents,
    origin: Origin,
}

impl Message {
    fn new(contents: Contents, origin: Origin) -> Message {
        Message { contents: contents, origin: origin }
    }
}

enum Contents {
    Play,
    Pause,
    Stop,
    Skip,
    VDown,
    VUp,
    HowMany,
    DontDie,
}

enum Origin {
    LifeSupport,
    Input,
}

// ---------------------- non-thread functions ----------
fn play_song(sink: &Sink, song: &Path) -> Result<(), io::Error> {
    //! Add a song to the sink
    let file = io::BufReader::new(File::open(song).unwrap());
    let source = Decoder::new(file).unwrap();
    sink.append(source);
    Ok(())
}

fn shuffle_and_queue(sink: &Sink, songs: &mut [PathBuf]) {
    let mut rng = thread_rng();
    songs.shuffle(&mut rng);
    for song in songs {
        let song_cow = song.as_path();
        if let Err(_e) = play_song(sink, song_cow) { 
            println!("song {} unable to be added to the queue.", song_cow.display())
        }
    }
}

// ------------------ Thread functions -------------------
fn player(to_input: Sender<usize>, to_ls: Sender<usize>, rcv: Receiver<Message>, sink: Sink, songs: &mut [PathBuf]) -> Result<(), io::Error> {
    //! thread fn to run the sink, waits for input in the form
    //! of a message to reciever telling it what to do.
    //! starts by queing all the songs.
    shuffle_and_queue(&sink, songs);
    let mut running = true;
    while running {
        let msg = rcv.recv().unwrap();
        match msg.contents {
            Contents::Play => sink.play(),
            Contents::Pause => sink.pause(),
            Contents::Skip => sink.skip_one(),
            Contents::Stop => { running = false; sink.stop(); }
            Contents::VDown => {
                let volume = sink.volume();
                if volume >= VOLUME_ADJUSTMENT { 
                    sink.set_volume(volume - VOLUME_ADJUSTMENT);
                } else {
                    sink.set_volume(0f32);
                }
            },
            Contents::VUp => {
                let volume = sink.volume(); 
                sink.set_volume(volume + VOLUME_ADJUSTMENT); 
            },
            Contents::DontDie => shuffle_and_queue(&sink, songs),
            Contents::HowMany => {
                // send the length of the queue back to the right place.
                match msg.origin {
                    Origin::LifeSupport => to_ls.send(sink.len()).unwrap(),
                    Origin::Input => to_input.send(sink.len()).unwrap(),
                }
            },
        };
    }

    Ok(())
}

fn input_thread(to: Sender<Message>, rcv: Receiver<usize>) -> Result<(), io::Error> {
    //! input thread function, prompts the user for input and 
    //! receives said input.
    let stdin = io::stdin();
    let mut buf = String::new();
    loop {
        if let Err(_e) = to.send(Message::new(Contents::HowMany, Origin::LifeSupport)) {
            break;
        }
        print!("Actions: \n
                 \t* p: play\n
                 \t* =: pause\n
                 \t* stop: stop. quit. end..\n
                 \t* s: skip\n
                 \t* +: volume up by 10%, can go above 100%\n
                 \t* -: volume down by 10%, min 0%.\n
                 \t* q: how many songs are in the queue\n
                 (type): ");
        stdin.read_line(&mut buf)?;
        match buf.as_str() {
            "p" => to.send(Message::new(Contents::Play, Origin::Input)).unwrap(),
            "=" => to.send(Message::new(Contents::Pause, Origin::Input)).unwrap(), 
            "stop" => to.send(Message::new(Contents::Stop, Origin::Input)).unwrap(), 
            "s" => to.send(Message::new(Contents::Skip, Origin::Input)).unwrap(), 
            "+" => to.send(Message::new(Contents::VUp, Origin::Input)).unwrap(), 
            "-" => to.send(Message::new(Contents::VDown, Origin::Input)).unwrap(), 
            "q" => { 
                to.send(Message::new(Contents::HowMany, Origin::Input)).unwrap();
                println!( "there are {} songs until they will be shuffled again.", rcv.recv().unwrap() ) 
            }, 
            &_ => todo!(),
        }
    }
    Ok(())
}

fn life_support_thread(to: Sender<Message>, rcv: Receiver<usize>) -> Result<(), ()> {
    //! keeps the players queue alive.
    loop {
        if let Err(_e) = to.send(Message::new(Contents::HowMany, Origin::LifeSupport)) {
            break;
        }
        let length = rcv.recv().unwrap();
        if length <= 1 {
            let _ = to.send(Message::new(Contents::DontDie, Origin::LifeSupport));
        }
        thread::sleep(Duration::from_secs(KEEP_ALIVE_TIME));
    }
    Ok(())
}

// ------------------ Main --------------------

fn main() -> Result<(), io::Error> {
    // take args
    let args: Vec<String> = env::args().collect();
    let dir_path = &args[1];
    println!("shuffling directory: {}", dir_path);
    
    // shuffle songs
    let mut paths = read_dir(dir_path).unwrap()
        .map(|dir| dir.map(|f| f.path()))
        .collect::<Result<Vec<_>, io::Error>>()?;
    
    // open output stream.
    let (_stream, stream_handle) = OutputStream::try_default().unwrap();
    let sink = Sink::try_new(&stream_handle).unwrap();

    // open thread resources
    let (to_player, rcv_player) = channel::<Message>();
    let (to_life_support, rcv_queue) = channel::<usize>();
    let (to_input, rcv_input) = channel::<usize>();

    // open stdin for taking input
    let player_handle = thread::spawn(move | | {
        let _ = player(to_input, to_life_support, rcv_player, sink, paths.as_mut_slice());
    });
    let to_player_clone = to_player.clone();
    let input_handle = thread::spawn(move | | {
        let _ = input_thread(to_player_clone, rcv_input);
    });
    let life_support_handle = thread::spawn(move | | {
        let _ = life_support_thread(to_player, rcv_queue);
    });
    
    player_handle.join().expect("player thread errored");
    input_handle.join().expect("input thread errored");
    life_support_handle.join().expect("life support errored");
    Ok(())
}
