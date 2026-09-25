// Hilda works the forge. (What she says when you talk to her is in
// dialogue/starting_town.yarn; this is what everyone nearby sees.)

fn on_idle(npc) {
    if chance(0.3) {
        npc.say("*clang* *clang*");
    }
}
