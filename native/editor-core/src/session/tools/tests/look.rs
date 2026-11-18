//! How the map looks under the mouse for each tool.

use super::*;

#[test]
fn the_cell_under_the_cursor_is_outlined_in_the_tool_s_colour() {
    let (mut session, _engine) = Session::offline(&scene());
    let look = |session: &mut Session, tool: Tool| {
        session.set_tool(tool);
        session.hover_look()
    };
    assert_eq!(look(&mut session, Tool::Paint), HoverLook { edge: [0, 255, 0, 255], fill: None, dashed: false, cross: false });
    let erase = look(&mut session, Tool::Erase);
    assert_eq!((erase.edge, erase.cross), ([0xff, 0x44, 0x44, 255], true));
    let pick = look(&mut session, Tool::Pick);
    assert_eq!((pick.edge, pick.dashed), ([0xff, 0xff, 0x00, 255], true));
    assert!(look(&mut session, Tool::Fill).fill.is_some());
    assert_eq!(look(&mut session, Tool::Line).edge, [0xff, 0xcc, 0x00, 255]);
    assert!(look(&mut session, Tool::Area).dashed);

    // The cable tool in the colour of the cable it lays, and the pipe tool in its pipe's.
    session.choose_cable(CableKind::Apc);
    assert_eq!(session.hover_look().edge, [0x00, 0xcc, 0x44, 255]);
    session.choose_pipe(PipeNetwork::Gas, Some("#990000FF".into()));
    assert_eq!(session.hover_look().edge, [0x99, 0x00, 0x00, 255]);

    // Place is green, and blue with Shift.
    session.set_tool(Tool::Place);
    let green = session.hover_look().edge;
    session.held.shift = true;
    assert_ne!(session.hover_look().edge, green);
}

#[test]
fn the_pointer_is_a_crosshair_an_arrow_or_a_hand() {
    let (mut session, _engine) = Session::offline(&scene());
    session.set_tool(Tool::Paint);
    assert_eq!(session.pointer_look(), PointerLook::Crosshair);
    session.set_tool(Tool::Select);
    assert_eq!(session.pointer_look(), PointerLook::Default);
    session.held.space = true;
    assert_eq!(session.pointer_look(), PointerLook::Grab);
    session.held.space = false;

    // The pan tool's hand closes while it pans.
    session.set_tool(Tool::Pan);
    assert_eq!(session.pointer_look(), PointerLook::Grab);
    session.pointer_down((320.0, 320.0), VIEW, Button::Primary);
    assert_eq!(session.pointer_look(), PointerLook::Grabbing);
    session.pointer_up((320.0, 320.0), VIEW);
    assert_eq!(session.pointer_look(), PointerLook::Grab);
}
