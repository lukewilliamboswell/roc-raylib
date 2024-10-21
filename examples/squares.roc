app [main, Model] {
    ray: platform "../platform/main.roc",
}

import ray.RocRay exposing [Rectangle, PlatformState]
import ray.RocRay.Keys as Keys

Program : {
    init : Task Model {},
    render : Model, PlatformState -> Task Model {},
}

Model : {
    squares : List Rectangle,
    status : [Ready, AfterClick RocRay.Vector2],
    circlePos : RocRay.Vector2,
}

width = 900
height = 400

main : Program
main = { init, render }

init : Task Model {}
init =

    RocRay.setWindowSize! { width, height }
    RocRay.setWindowTitle! "Squares Demo"

    Task.ok {
        circlePos: { x: width / 2, y: height / 2 },
        squares: [],
        status: Ready,
    }

render : Model, PlatformState -> Task Model {}
render = \model, { keys, mouse } ->

    RocRay.beginDrawing! Black

    RocRay.drawText! { pos: { x: width - 400, y: height - 25 }, text: "Click on the screen ...", size: 20, color: White }

    mousePos = mouse.position

    RocRay.drawText! {
        pos: {
            x: 10,
            y: height - 25,
        },
        text: "Mouse $(Num.toStr mousePos.x),$(Num.toStr mousePos.y), $(Inspect.toStr keys), $(Inspect.toStr mouse.buttons)",
        size: 20,
        color: White,
    }

    RocRay.drawRectangle! { rect: { x: Num.toF32 mousePos.x - 10, y: Num.toF32 mousePos.y - 10, width: 20, height: 20 }, color: Red }

    RocRay.drawRectangle! { rect: { x: model.circlePos.x, y: model.circlePos.y, width: 50, height: 50 }, color: Aqua }

    RocRay.endDrawing!

    newCirclePos =
        if Keys.down keys KeyUp then
            { x: model.circlePos.x, y: model.circlePos.y - 10 }
        else if Keys.down keys KeyDown then
            { x: model.circlePos.x, y: model.circlePos.y + 10 }
        else if Keys.down keys KeyLeft then
            { x: model.circlePos.x - 10, y: model.circlePos.y }
        else if Keys.down keys KeyRight then
            { x: model.circlePos.x + 10, y: model.circlePos.y }
        else
            model.circlePos

    Task.ok { model & circlePos: newCirclePos }
