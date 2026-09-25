// Guard Aldren keeps watch over the town square. (What he says when you talk
// to him is in dialogue/starting_town.yarn; this is what everyone nearby sees.)

fn on_idle(npc) {
    if chance(0.15) {
        npc.say("All quiet.");
    }
}
