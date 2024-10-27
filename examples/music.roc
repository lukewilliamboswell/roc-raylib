app [Model, init, render] { rr: platform "../platform/main.roc" }

# https://www.raylib.com/examples/audio/loader.html?name=audio_music_stream

import rr.RocRay exposing [Rectangle]
import rr.Music exposing [Music]
import rr.Draw

Model : {
    track : Music,
    trackState : TrackState,
}

TrackState : [Stopped, Playing]

init : Task Model []
init =
    RocRay.initWindow! { title: "Music" }

    track = Music.load! "examples/assets/music/benny-hill.mp3"

    Task.ok { track, trackState: Stopped }

render : Model, RocRay.PlatformState -> Task Model []
render = \model, _state ->
    trackState = Playing

    newModel = { model & trackState }

    draw! newModel

    when (model.trackState, trackState) is
        (Stopped, Playing) ->
            Music.play! model.track
            Task.ok newModel

        _ ->
            Task.ok newModel

draw : Model -> Task {} []
draw = \_model ->
    Draw.draw! White \{} ->
        Draw.text! {
            text: "Music should be playing!",
            size: 20,
            color: Gray,
            pos: { x: 100, y: 100 },
        }

        bar : Rectangle
        bar = {
            x: 100.0,
            y: 150.0,
            width: 600.0,
            height: 20.0,
        }

        border : F32
        border = 1.0

        # border (to be drawn over)
        Draw.rectangle! {
            rect: {
                x: bar.x - border,
                y: bar.y - border,
                width: bar.width + border * 2,
                height: bar.height + border * 2,
            },
            color: Black,
        }

        # background
        Draw.rectangle! {
            rect: bar,
            color: Silver,
        }

        # progress
        Draw.rectangle! {
            rect: { bar & width: bar.width },
            color: Red,
        }

        Draw.text! {
            text: "PRESS SPACE TO RESTART MUSIC",
            size: 20,
            color: Gray,
            pos: { x: 100, y: 300 },
        }

        Draw.text! {
            text: "PRESS P TO PAUSE/RESUME MUSIC",
            size: 20,
            color: Gray,
            pos: { x: 100, y: 350 },
        }

    Task.ok {}
