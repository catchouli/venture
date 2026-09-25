// Elsie bakes for the whole town.

fn on_spawn(npc) {
    npc.say("Ovens are warming up!");
}

fn on_interact(npc, player) {
    npc.say("Fresh bread tomorrow, " + player.name + ". I'll save you a loaf!");
}

fn on_idle(npc) {
    if chance(0.5) {
        npc.wander();
    }
}
