use std::path::Path;

use expand::expand;
use eyre::{bail, Context, Result};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{ tcp, unix, TcpStream, ToSocketAddrs, UnixStream },
};

pub struct Client<R, W> {
    r: BufReader<R>,
    w: W,
}

#[derive(Debug, Eq, PartialEq)]
pub enum PlayerState {
    Play,
    Pause,
    Stop,
}

#[derive(Debug)]
pub struct Status {
    pub repeat: bool,
    pub random: bool,
    pub single: Option<bool>,
    pub consume: bool,
    pub queue_len: usize,
    pub state: PlayerState,
    pub song: Option<Song>,
}

#[derive(Debug)]
pub struct Song {
    pub pos: usize,
    pub elapsed: u16,
}

#[derive(Debug)]
pub struct Track {
    pub file: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub title: Option<String>,
    pub time: u16,
}

impl<R, W> Client<R, W>
where
    R: AsyncReadExt + Unpin,
    W: AsyncWriteExt + Unpin,
{
    pub async fn init_tcp_client(addr: impl ToSocketAddrs) -> Result<Client<tcp::OwnedReadHalf, tcp::OwnedWriteHalf>> {
        let (r, w) = TcpStream::connect(addr).await?.into_split();
        let client = Client {
            r: BufReader::new(r),
            w,
        };

        client.init().await
    }

    pub async fn init_sock_client(addr: impl AsRef<Path>) -> Result<Client<unix::OwnedReadHalf, unix::OwnedWriteHalf>> {
        let (r, w) = UnixStream::connect(addr).await?.into_split();
        let client = Client {
            r: BufReader::new(r),
            w,
        };

        client.init().await
    }
    pub async fn init(mut self) -> Result<Client<R, W>> {
        let buf = &mut [0; 7];
        let _ = self.r.read(buf).await?;
        if buf != b"OK MPD " {
            bail!("server did not greet with a success");
        }
        self.r.read_line(&mut String::with_capacity(8)).await?;
        Ok(self)
    }

    pub async fn idle(&mut self) -> Result<(bool, bool)> {
        async move {
            self.w.write_all(b"idle options player playlist\n").await?;
            let mut lines = (&mut self.r).lines();
            let mut status = false;
            let mut queue = false;

            while let Ok(Some(line)) = lines.next_line().await {
                match line.as_bytes() {
                    b"changed: options" => status = true,
                    b"changed: player" => status = true,
                    b"changed: playlist" => queue = true,
                    b"OK" => break,
                    _ => continue,
                }
            }

            Result::<_>::Ok((status, queue))
        }
        .await
        .context("Failed to idle")
    }

    pub async fn queue(&mut self, len: usize) -> Result<Vec<Track>> {
        async move {
            let mut first = true;
            let mut tracks = Vec::with_capacity(len);

            let mut file = None;
            let mut artist = None;
            let mut album = None;
            let mut title = None;
            let mut time = 0;

            self.w.write_all(b"playlistinfo\n").await?;
            let mut lines = (&mut self.r).lines();

            while let Ok(Some(line)) = lines.next_line().await {
                match line.as_bytes() {
                    b"OK" => break,
                    expand!([@b"file: ", ..]) => {
                        if first {
                            first = false;
                        } else if let Some(file) = file {
                            let track = Track {
                                file,
                                artist,
                                album,
                                title,
                                time,
                            };
                            tracks.push(track);
                        } else {
                            bail!("incomplete playlist response");
                        }

                        file = Some(line[6..].into());
                        artist = None;
                        album = None;
                        title = None;
                        time = 0;
                    }
                    expand!([@b"Artist: ", ..]) => artist = Some(line[8..].into()),
                    expand!([@b"Album: ", ..]) => album = Some(line[7..].into()),
                    expand!([@b"Title: ", ..]) => title = Some(line[7..].into()),
                    expand!([@b"Time: ", ..]) => time = line[6..].parse()?,
                    _ => continue,
                }
            }

            if let Some(file) = file {
                let track = Track {
                    file,
                    artist,
                    album,
                    title,
                    time,
                };
                tracks.push(track);
            }

            Ok(tracks)
        }
        .await
        .context("Failed to query queue")
    }

    pub async fn status(&mut self) -> Result<Status> {
        async move {
            let mut repeat = None;
            let mut random = None;
            let mut single = None;
            let mut consume = None;
            let mut queue_len = None;
            let mut state = PlayerState::Stop;
            let mut pos = None;
            let mut elapsed = None;

            self.w.write_all(b"status\n").await?;
            let mut lines = (&mut self.r).lines();

            while let Ok(Some(line)) = lines.next_line().await {
                match line.as_bytes() {
                    b"OK" => break,
                    b"repeat: 0" => repeat = Some(false),
                    b"repeat: 1" => repeat = Some(true),
                    b"random: 0" => random = Some(false),
                    b"random: 1" => random = Some(true),
                    b"single: 0" => single = Some(Some(false)),
                    b"single: 1" => single = Some(Some(true)),
                    b"single: oneshot" => single = Some(None),
                    b"consume: 0" => consume = Some(false),
                    b"consume: 1" => consume = Some(true),
                    expand!([@b"playlistlength: ", ..]) => queue_len = Some(line[16..].parse()?),
                    b"state: play" => state = PlayerState::Play,
                    b"state: pause" => state = PlayerState::Pause,
                    expand!([@b"song: ", ..]) => pos = Some(line[6..].parse()?),
                    expand!([@b"elapsed: ", ..]) => {
                        elapsed = Some(line[9..].parse::<f32>()?.round() as u16)
                    }
                    _ => continue,
                }
            }

            if let (Some(repeat), Some(random), Some(single), Some(consume), Some(queue_len)) =
                (repeat, random, single, consume, queue_len)
            {
                Ok(Status {
                    repeat,
                    random,
                    single,
                    consume,
                    queue_len,
                    state,
                    song: if let (Some(pos), Some(elapsed)) = (pos, elapsed) {
                        Some(Song { pos, elapsed })
                    } else {
                        None
                    },
                })
            } else {
                bail!("incomplete status response");
            }
        }
        .await
        .context("Failed to query status")
    }

    pub async fn play(&mut self, pos: usize) -> Result<()> {
        self.w.write_all(b"play ").await?;
        self.w.write_all(pos.to_string().as_bytes()).await?;
        self.w.write_all(b"\n").await?;
        let mut lines = (&mut self.r).lines();

        while let Ok(Some(line)) = lines.next_line().await {
            match line.as_bytes() {
                b"OK" | expand!([@b"ACK ", ..]) => break,
                _ => continue,
            }
        }

        Ok(())
    }

    pub async fn command(&mut self, cmd: &[u8]) -> Result<()> {
        self.w.write_all(cmd).await?;
        self.w.write_all(b"\n").await?;
        let mut lines = (&mut self.r).lines();

        while let Ok(Some(line)) = lines.next_line().await {
            match line.as_bytes() {
                b"OK" | expand!([@b"ACK ", ..]) => break,
                _ => continue,
            }
        }

        Ok(())
    }
}
