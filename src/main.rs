//! Example demonstrating how to make use of individual track audio events,
//! and how to use the `TrackQueue` system.
//!
//! Requires the "cache", "standard_framework", and "voice" features be enabled in your
//! Cargo.toml, like so:
//!
//! ```toml
//! [dependencies.serenity]
//! git = "https://github.com/serenity-rs/serenity.git"
//! features = ["cache", "framework", "standard_framework", "voice"]
//! ```

use rand::seq::SliceRandom;
use rand::thread_rng;
use serde::{Deserialize, Serialize};

use std::{collections::HashSet, env, time::Duration};

use serenity::{
    async_trait,
    client::{Client, Context, EventHandler},
    framework::{
        standard::{
            help_commands,
            macros::{command, group, help},
            Args, CommandGroup, CommandResult, HelpOptions,
        },
        StandardFramework,
    },
    model::{
        channel::Message,
        gateway::Ready,
        id::{ChannelId, GuildId, UserId},
    },
    Result as SerenityResult,
};

use songbird::{
    input::restartable::Restartable, tracks::PlayMode, Event, EventContext, SerenityInit,
    TrackEvent,
};

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
    }
}

#[group]
#[commands(
    deafen, mute, queue, skip, stop, undeafen, unmute, join, pause, resume, shuffle, play
)]
struct General;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    // Configure the client with your Discord bot token in the environment.
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

    let framework = StandardFramework::new()
        .configure(|c| c.prefix("~"))
        .help(&MY_HELP)
        .group(&GENERAL_GROUP);

    let mut client = Client::builder(&token)
        .event_handler(Handler)
        .framework(framework)
        .register_songbird()
        .await
        .expect("Err creating client");

    let _ = client
        .start()
        .await
        .map_err(|why| println!("Client ended: {:?}", why));
}

#[command]
async fn deafen(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    let handler_lock = match manager.get(guild_id) {
        Some(handler) => handler,
        None => {
            check_msg(msg.reply(ctx, "Not in a voice channel").await);

            return Ok(());
        }
    };

    let mut handler = handler_lock.lock().await;

    if handler.is_deaf() {
        check_msg(msg.channel_id.say(&ctx.http, "Already deafened").await);
    } else {
        if let Err(e) = handler.deafen(true).await {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, format!("Failed: {:?}", e))
                    .await,
            );
        }

        check_msg(msg.channel_id.say(&ctx.http, "Deafened").await);
    }

    Ok(())
}

#[help]
#[command_not_found_text = "Could not find: `{}`."]
#[max_levenshtein_distance(3)]
#[lacking_permissions = "Strike"]
#[lacking_role = "Hide"]
#[wrong_channel = "Strike"]
async fn my_help(
    context: &Context,
    msg: &Message,
    args: Args,
    help_options: &'static HelpOptions,
    groups: &[&'static CommandGroup],
    owners: HashSet<UserId>,
) -> CommandResult {
    let _ = help_commands::with_embeds(context, msg, args, help_options, groups, owners).await;
    Ok(())
}

#[command]
#[only_in(guilds)]
async fn join(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let channel_id = guild
        .voice_states
        .get(&msg.author.id)
        .and_then(|voice_state| voice_state.channel_id);

    let connect_to = match channel_id {
        Some(channel) => channel,
        None => {
            check_msg(msg.reply(ctx, "Not in a voice channel").await);

            return Ok(());
        }
    };

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    let (_, _) = manager.join(guild_id, connect_to).await;

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn leave(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();
    let has_handler = manager.get(guild_id).is_some();

    if has_handler {
        if let Err(e) = manager.remove(guild_id).await {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, format!("Failed: {:?}", e))
                    .await,
            );
        }

        check_msg(msg.channel_id.say(&ctx.http, "Left voice channel").await);
    } else {
        check_msg(msg.reply(ctx, "Not in a voice channel").await);
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn mute(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    let handler_lock = match manager.get(guild_id) {
        Some(handler) => handler,
        None => {
            check_msg(msg.reply(ctx, "Not in a voice channel").await);

            return Ok(());
        }
    };

    let mut handler = handler_lock.lock().await;

    if handler.is_mute() {
        check_msg(msg.channel_id.say(&ctx.http, "Already muted").await);
    } else {
        if let Err(e) = handler.mute(true).await {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, format!("Failed: {:?}", e))
                    .await,
            );
        }

        check_msg(msg.channel_id.say(&ctx.http, "Now muted").await);
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn play(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    stop(ctx, msg, args.clone()).await?;
    queue(ctx, msg, args).await?;
    Ok(())
}
#[command]
#[only_in(guilds)]
async fn queue(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let url = args.raw_quoted().collect::<Vec<&str>>().join(" ");
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    let handler_lock = if let Some(handler_lock) = manager.get(guild_id) {
        handler_lock
    } else {
        let guild = msg.guild(&ctx.cache).await.unwrap();
        let guild_id = guild.id;

        let channel_id = guild
            .voice_states
            .get(&msg.author.id)
            .and_then(|voice_state| voice_state.channel_id);

        let connect_to = match channel_id {
            Some(channel) => channel,
            None => {
                check_msg(msg.reply(ctx, "Not in a voice channel").await);

                return Ok(());
            }
        };

        let manager = songbird::get(ctx)
            .await
            .expect("Songbird Voice client placed in at initialisation.")
            .clone();

        let (handle_lock, _) = manager.join(guild_id, connect_to).await;
        handle_lock
    };

    let mut handler = handler_lock.lock().await;

    // Here, we use lazy restartable sources to make sure that we don't pay
    // for decoding, playback on tracks which aren't actually live yet.
    let sources = if !url.starts_with("http") {
        match Restartable::ytdl_search(url, true).await {
            Ok(source) => vec![source],
            Err(why) => {
                println!("Err starting source: {:?}", why);

                check_msg(msg.channel_id.say(&ctx.http, "Error sourcing ffmpeg").await);

                return Ok(());
            }
        }
    } else if url.starts_with("https://www.youtube.com/playlist?list=") {
        let mut sources = Vec::new();
        let client = reqwest::Client::builder()
            .user_agent("User agent: timothee.leberre@gmail.com")
            .build()
            .unwrap();
        let playlist_id = url
            .strip_prefix("https://www.youtube.com/playlist?list=")
            .unwrap();
        let url = format!("https://www.googleapis.com/youtube/v3/playlistItems?part=snippet&maxResults=100&playlistId={}&key={}", playlist_id, env::var("GOOGLE_TOKEN").expect("Expected a token in the environment"));
        let resp = client.get(url).send().await?.json::<Playlist>().await?;
        for item in resp.items {
            let url = format!(
                "https://www.youtube.com/watch?v={}",
                item.snippet.resourceId.videoId
            );
            match Restartable::ytdl(url, true).await {
                Ok(source) => sources.push(source),
                Err(why) => {
                    println!("Err starting source: {:?}", why);
                }
            }
        }
        sources
    } else {
        match Restartable::ytdl(url, true).await {
            Ok(source) => vec![source],
            Err(why) => {
                println!("Err starting source: {:?}", why);

                check_msg(msg.channel_id.say(&ctx.http, "Error sourcing ffmpeg").await);

                return Ok(());
            }
        }
    };
    let n = sources.len();
    for source in sources {
        handler.enqueue_source(source.into());
    }

    let guild_id = msg.guild_id.unwrap();
    let chan_id = msg.channel_id;

    handler.add_global_event(
        Event::Track(TrackEvent::End),
        TrackEndNotifier {
            guild_id,
            ctx: ctx.clone(),
        },
    );

    handler.add_global_event(
        Event::Delayed(Duration::from_secs(7200)),
        DurationElapsedNotifier {
            guild_id,
            chan_id,
            quit: false,
            ctx: ctx.clone(),
        },
    );

    check_msg(
        msg.channel_id
            .say(
                &ctx.http,
                format!(
                    "Added {} songs to queue: total length is {}",
                    n,
                    handler.queue().len()
                ),
            )
            .await,
    );

    Ok(())
}

struct TrackEndNotifier {
    guild_id: GuildId,
    ctx: Context,
}

#[async_trait]
impl songbird::EventHandler for TrackEndNotifier {
    async fn act(&self, _: &EventContext<'_>) -> Option<Event> {
        let manager = songbird::get(&self.ctx)
            .await
            .expect("Songbird Voice client placed in at initialisation.")
            .clone();
        if if let Some(handler_lock) = manager.get(self.guild_id) {
            let handler = handler_lock.lock().await;
            let queue = handler.queue();

            if queue.is_empty() {
                true
            } else {
                false
            }
        } else {
            false
        } {
            manager.remove(self.guild_id).await.unwrap();
        };

        None
    }
}

struct DurationElapsedNotifier {
    guild_id: GuildId,
    chan_id: ChannelId,
    quit: bool,
    ctx: Context,
}

#[async_trait]
impl songbird::EventHandler for DurationElapsedNotifier {
    async fn act(&self, _: &EventContext<'_>) -> Option<Event> {
        if !self.quit {
            let manager = songbird::get(&self.ctx)
                .await
                .expect("Songbird Voice client placed in at initialisation.")
                .clone();
            if let Some(handler_lock) = manager.get(self.guild_id) {
                let mut handler = handler_lock.lock().await;
                handler.add_global_event(
                    Event::Delayed(Duration::from_secs(300)),
                    DurationElapsedNotifier {
                        guild_id: self.guild_id,
                        chan_id: self.chan_id,
                        quit: true,
                        ctx: self.ctx.clone(),
                    },
                );
                let queue = handler.queue();
                let _ = queue.pause();
            };
            self.chan_id
                .say(&self.ctx, "Paused content after 2 hours. ~resume to resume")
                .await
                .unwrap();
        } else {
            let manager = songbird::get(&self.ctx)
                .await
                .expect("Songbird Voice client placed in at initialisation.")
                .clone();
            if if let Some(handler_lock) = manager.get(self.guild_id) {
                let handler = handler_lock.lock().await;
                let queue = handler.queue();
                if queue.current().unwrap().get_info().await.unwrap().playing == PlayMode::Pause {
                    true
                } else {
                    false
                }
            } else {
                false
            } {
                let _ = manager.remove(self.guild_id).await;
            }
        }

        None
    }
}

#[command]
#[only_in(guilds)]
async fn skip(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let handler = handler_lock.lock().await;
        let queue = handler.queue();
        let _ = queue.skip();

        check_msg(
            msg.channel_id
                .say(
                    &ctx.http,
                    format!("Song skipped: {} in queue.", queue.len()),
                )
                .await,
        );
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to play in")
                .await,
        );
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn shuffle(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let handler = handler_lock.lock().await;
        let queue = handler.queue();
        let _ = queue.pause();
        let _ = queue.modify_queue(|q| q.make_contiguous().shuffle(&mut thread_rng()));
        let _ = queue.resume();
        check_msg(msg.channel_id.say(&ctx.http, "Shuffled the queue").await);
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to play in")
                .await,
        );
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn pause(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let handler = handler_lock.lock().await;
        let queue = handler.queue();
        let _ = queue.pause();

        check_msg(msg.channel_id.say(&ctx.http, "Paused the queue").await);
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to play in")
                .await,
        );
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn resume(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let handler = handler_lock.lock().await;
        let queue = handler.queue();
        let _ = queue.resume();

        check_msg(msg.channel_id.say(&ctx.http, "Resumed the queue").await);
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to play in")
                .await,
        );
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn stop(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let handler = handler_lock.lock().await;
        let queue = handler.queue();
        let _ = queue.stop();

        check_msg(msg.channel_id.say(&ctx.http, "Queue cleared.").await);
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to play in")
                .await,
        );
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn undeafen(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;
        if let Err(e) = handler.deafen(false).await {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, format!("Failed: {:?}", e))
                    .await,
            );
        }

        check_msg(msg.channel_id.say(&ctx.http, "Undeafened").await);
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to undeafen in")
                .await,
        );
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn unmute(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;
    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;
        if let Err(e) = handler.mute(false).await {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, format!("Failed: {:?}", e))
                    .await,
            );
        }

        check_msg(msg.channel_id.say(&ctx.http, "Unmuted").await);
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to unmute in")
                .await,
        );
    }

    Ok(())
}

/// Checks that a message successfully sent; if not, then logs why to stdout.
fn check_msg(result: SerenityResult<Message>) {
    if let Err(why) = result {
        println!("Error sending message: {:?}", why);
    }
}

#[derive(Default, Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
#[allow(non_snake_case)]
pub struct Playlist {
    //pub nextPageToken: String,
    pub items: Vec<Item>,
}

#[derive(Default, Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
#[allow(non_snake_case)]
pub struct Item {
    pub snippet: Snippet,
}

#[derive(Default, Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
#[allow(non_snake_case)]
pub struct Snippet {
    pub resourceId: RessourceId,
}

#[derive(Default, Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
#[allow(non_snake_case)]
pub struct RessourceId {
    pub videoId: String,
}
