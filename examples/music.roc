app [Model, init!, render!] { rr: platform "../platform/main.roc" }

# https://www.raylib.com/examples/audio/loader.html?name=audio_music_stream

import rr.RocRay exposing [Rectangle]
import rr.Music exposing [Music]
import rr.Draw
import rr.Keys

Model : {
    track : Music,
    track_state : TrackState,
}

TrackState : [Init, Playing, Paused]

Intent : [Restart, TogglePause, Continue]

init! : {} => Result Model _
init! = |{}|
    RocRay.init_window!({ title: "Music" })

    track = try(Music.load!, "examples/assets/music/benny-hill.mp3")

    Ok({ track, track_state: Init })

render! : Model, RocRay.PlatformState => Result Model []
render! = |model, { keys }|
    intent =
        if Keys.pressed(keys, KeySpace) then
            Restart
        else if Keys.pressed(keys, KeyP) then
            TogglePause
        else
            Continue

    track_state = update_music!(intent, model.track_state, model.track)

    new_model = { model & track_state }

    draw!(new_model)

    Ok(new_model)

update_music! : Intent, TrackState, Music => TrackState
update_music! = |intent, track_state, track|
    when (intent, track_state) is
        (Restart, Paused) ->
            # the extra initial `Music.play!` is necesssary to get raylib to reset
            # the time played on the track
            # this avoids a bug in their example linked above
            Music.play!(track)
            Music.stop!(track)
            Music.play!(track)
            Playing

        (Restart, _) ->
            Music.stop!(track)
            Music.play!(track)
            Playing

        (TogglePause, Playing) ->
            Music.pause!(track)
            Paused

        (TogglePause, Paused) ->
            Music.play!(track)
            Playing

        (TogglePause, Init) ->
            Music.play!(track)
            Playing

        (Continue, Init) ->
            Music.play!(track)
            Playing

        _ ->
            track_state

draw! : Model => {}
draw! = |model|
    time_played = Music.get_time_played!(model.track)
    length = Music.length(model.track)
    progress = time_played / length

    Draw.draw!(
        White,
        |{}|
            Draw.text!(
                {
                    text: "Music should be playing!",
                    size: 20,
                    color: Gray,
                    pos: { x: 100, y: 100 },
                },
            )

            bar : Rectangle
            bar = {
                x: 100.0,
                y: 150.0,
                width: 600.0,
                height: 20.0,
            }

            border : F32
            border = 1.0

            # border (as rect to be drawn over)
            border_rect = {
                x: bar.x - border,
                y: bar.y - border,
                width: bar.width + border * 2,
                height: bar.height + border * 2,
            }
            Draw.rectangle!({ rect: border_rect, color: Black })

            # background
            Draw.rectangle!({ rect: bar, color: Silver })

            # progress
            progress_rect = { bar & width: bar.width * progress }
            Draw.rectangle!({ rect: progress_rect, color: Red })

            Draw.text!(
                {
                    text: "PRESS SPACE TO RESTART MUSIC",
                    size: 20,
                    color: Gray,
                    pos: { x: 100, y: 300 },
                },
            )

            Draw.text!(
                {
                    text: "PRESS P TO PAUSE/RESUME MUSIC",
                    size: 20,
                    color: Gray,
                    pos: { x: 100, y: 350 },
                },
            ),
    )
