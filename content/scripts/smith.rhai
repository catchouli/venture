// Hilda works the forge.

fn on_interact(npc, player) {
    npc.say("Welcome, " + player.name + "! My forge isn't lit yet. Come back soon and I'll have blades to sell.");
}

fn on_idle(npc) {
    if chance(0.3) {
        npc.say("*clang* *clang*");
    }
}
