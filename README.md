# Battlerust
## What is it
This is a battleengine written as a library or standalone for simmulating fights for ogame/ogame like games.


## Features 
This is a battleengine written as a library or standalone for simmulating fights for ogame/ogame like games.
he engine itself has no knowlege about ship values or so, this has to be all given over and is highly configurable.
Thus this can indroduce a big veriaty of new ships easily.
Basic features:
- explode: ships have a chance to explode after a certain percentage of hull remaining on each shot, configurable percentage for every shiptype
- rapidfire: chance to shoot again after hitting a ship 
- shieldbounce: when the attack is too low it is getting ignored when the shield is still up. (configurable)
- bitflags: flags that allow extra features on a shipbasis like stunning and escaping the battle.
- Multithreading: attackers and defenders shoot in realtime against eachother.
Additional features:
- Lots of statistics, on who hit what so accurate analysis of fights can be made
- perround: option to set all values per round and not only for the whole fight, if it is set for less rounds the last value is taken for the rest
- bigshield: allows to set shields as "shipshields" (thoguht for shield domes). These shields has to be taken offline before any other ships can be targeted.
- stun: abillity to stun ships hit, configurable with a value, of stunchance *(attack/enemy_hull) > random
- escape: chance for ships to escape a battle if hit and brought under a certain percentage of hull. Only triggers if survives the round and is then removed.
- shrapnel: chance for hits to do N additional hits with M damage each (they don't trigger rapidfire)
- rfcancel: check for ships being still alive to cancel rapidfire when the enemy fleet is already dead.


## How to compile and use
normal rust compilation, use the flags and features wanted (it doesn't make a real performance difference)
for integration of the library via ffi look at https://codeberg.org/pr0game



