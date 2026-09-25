// Elsie bakes for the whole town. (What she says when you talk to her is in
// dialogue/starting_town.yarn; this is what everyone nearby sees.)

fn on_spawn(npc) {
    npc.say("Ovens are warming up!");
}

fn on_idle(npc) {
    if chance(0.5) {
        npc.wander();
    } else if chance(0.1) {
        npc.say("Fresh bread tomorrow!");
    }
}
