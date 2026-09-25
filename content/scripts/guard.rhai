// Guard Aldren keeps watch over the town square.

fn on_interact(npc, player) {
    let advice = [
        "Stay on the roads after dark, " + player.name + ". Wolves don't.",
        "The river's safe to wade where it's shallow. Mind the deep pools.",
        "If you see smoke to the south, that's the volcano. Don't go poking at it.",
    ];
    npc.say(advice[random(advice.len())]);
}

fn on_idle(npc) {
    if chance(0.15) {
        npc.say("All quiet.");
    }
}
