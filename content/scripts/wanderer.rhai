// Old Tomas ambles about the square, muttering to himself.

fn on_interact(npc, player) {
    let tales = [
        "When I was your age, " + player.name + ", these hills were all forest.",
        "They say the dwarves dig so deep they hear the world's heart beating.",
        "Elsie's bread is the best in three valleys. Don't tell her I said so.",
    ];
    npc.say(tales[random(tales.len())]);
}

fn on_idle(npc) {
    npc.wander();
    if chance(0.1) {
        npc.say("Hmm... where did I leave my pipe?");
    }
}
