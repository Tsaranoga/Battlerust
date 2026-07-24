use rand::prelude::IndexedRandom;
use std::time::Instant;
use serde::{Deserialize, Serialize, Deserializer};
use serde_json;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::collections::HashMap;
use crossbeam::thread;
use std::cell::UnsafeCell;
use rand::RngExt;
use wasm_bindgen::prelude::*;
use num_traits::ToPrimitive;

//flags
#[cfg(feature = "stun")]
const SELF_FLAG_STUNNED:u8=0;
#[cfg(feature = "stun")]
const OTHER_FLAG_STUNNING:u8=0;
#[cfg(feature = "escape")]
const OTHER_FLAG_ESCAPE:u8=1;

//these threadcells are used so we can use multithreading
pub struct ThreadCell<T>(pub UnsafeCell<T>);
impl<T> ThreadCell<T> {
    #[inline]
    pub fn set(&self, value: T) {
        unsafe {
            *self.0.get() = value;
        }
    }

    #[inline]
    pub fn get(&self) -> T where T: Copy {
        unsafe { *self.0.get() }
    }
}
unsafe impl<T> Sync for ThreadCell<T> {}

//structs to parse the input json
#[derive(Debug, Deserialize)]
pub struct ParseBattleData {
    pub attacker: Vec<ParseFleetSide>,
    pub defender: Vec<ParseFleetSide>,
    pub rounds: usize,
}

#[derive(Debug, Deserialize)]
pub struct ParseFleetSide {
    pub name: usize,
    pub ships: Vec<ParseShipInfo>,

}

fn float_or_vec<'de, D>(deserializer: D) -> Result<Vec<f32>, D::Error> where    D: Deserializer<'de>,{
    let value = serde_json::Value::deserialize(deserializer)?;

    match value {
        serde_json::Value::Number(num) => {
            let f = num.as_f64().ok_or_else(|| serde::de::Error::custom("invalid number"))? as f32;
            Ok(vec![f]) // single float -> vec with one element
        }
        serde_json::Value::Array(arr) => {
            let mut result = Vec::with_capacity(arr.len());
            for v in arr {
                match v {
                    serde_json::Value::Number(n) => {
                        result.push(n.as_f64().ok_or_else(|| serde::de::Error::custom("invalid number"))? as f32);
                    }
                    _ => return Err(serde::de::Error::custom("expected number")),
                }
            }
            Ok(result)
        }
        _ => Err(serde::de::Error::custom("expected float or array of floats")),
    }
}

#[cfg(feature = "bigshield")]
fn bool_or_vec<'de, D>(deserializer: D) -> Result<Vec<bool>, D::Error>where    D: Deserializer<'de>,{
    let value = serde_json::Value::deserialize(deserializer)?;

    match value {
        serde_json::Value::Bool(b) => Ok(vec![b]), // single bool -> vec with one element
        serde_json::Value::Array(arr) => {
            let mut result = Vec::with_capacity(arr.len());
            for v in arr {
                match v {
                    serde_json::Value::Bool(b) => result.push(b),
                    _ => return Err(serde::de::Error::custom("expected boolean")),
                }
            }
            Ok(result)
        }
        _ => Err(serde::de::Error::custom("expected bool or array of bools")),
    }
}

fn map_or_vec<'de, D>(deserializer: D) -> Result<Vec<HashMap<usize, f32>>, D::Error>where  D: Deserializer<'de>,{
    let value = serde_json::Value::deserialize(deserializer)?;

    // helper to convert a JSON object into HashMap<usize, f32>
    fn parse_map<E>(map: serde_json::Map<String, serde_json::Value>) -> Result<HashMap<usize, f32>, E>
    where
        E: serde::de::Error,
    {
        let mut result = HashMap::with_capacity(map.len());

        for (k, v) in map {
            let key: usize = k
                .parse()
                .map_err(|_| E::custom("invalid usize key"))?;

            let val = match v {
                serde_json::Value::Number(n) => n
                    .as_f64()
                    .ok_or_else(|| E::custom("invalid number"))? as f32,
                _ => return Err(E::custom("expected number")),
            };

            result.insert(key, val);
        }

        Ok(result)
    }

    match value {
        // single map → wrap in Vec
        serde_json::Value::Object(map) => {
            Ok(vec![parse_map::<D::Error>(map)?])
        }

        // array of maps
        serde_json::Value::Array(arr) => {
            let mut result = Vec::with_capacity(arr.len());

            for item in arr {
                match item {
                    serde_json::Value::Object(map) => {
                        result.push(parse_map::<D::Error>(map)?);
                    }
                    _ => return Err(serde::de::Error::custom("expected object")),
                }
            }

            Ok(result)
        }

        _ => Err(serde::de::Error::custom(
            "expected object or array of objects",
        )),
    }
}

#[cfg(feature = "shrapnel")]
fn usize_or_vec<'de, D>(deserializer: D) -> Result<Vec<usize>, D::Error> where    D: Deserializer<'de>,{
    let value = serde_json::Value::deserialize(deserializer)?;

    match value {
        serde_json::Value::Number(num) => {
            let f = num.as_u64().ok_or_else(|| serde::de::Error::custom("invalid number"))? as usize;
            Ok(vec![f]) // single float -> vec with one element
        }
        serde_json::Value::Array(arr) => {
            let mut result = Vec::with_capacity(arr.len());
            for v in arr {
                match v {
                    serde_json::Value::Number(n) => {
                        result.push(n.as_u64().ok_or_else(|| serde::de::Error::custom("invalid number"))? as usize);
                    }
                    _ => return Err(serde::de::Error::custom("expected number")),
                }
            }
            Ok(result)
        }
        _ => Err(serde::de::Error::custom("expected float or array of floats")),
    }
}


#[cfg(feature = "bigshield")]
fn default_false_vec() -> Vec<bool> {
    vec![false]
}

fn default_bounce() -> Vec<f32> {
    vec![0.01]
}
#[cfg(any(feature = "explode", feature = "stun",feature = "shrapnel",feature = "escape"))]
fn default_zero() -> Vec<f32> {
    vec![0.0]
}


fn default_rf() -> Vec<HashMap<usize, f32>> {
    vec![HashMap::new()]
}

#[cfg(feature = "shrapnel")]
fn default_shrapnel() -> Vec<usize> {
    vec![0]
}


#[derive(Debug, Deserialize)]
pub struct ParseShipInfo {
    pub shipid:usize,
    pub amount: usize,
    pub hull: f32,
    #[serde(default="default_rf", deserialize_with = "map_or_vec")]
    pub rapidfire: Vec<HashMap<usize, f32>>,
    #[serde(deserialize_with = "float_or_vec")]
    pub attack: Vec<f32>,
    #[serde(deserialize_with = "float_or_vec")]
    pub shield: Vec<f32>,
    #[cfg(feature = "explode")]
    #[serde(deserialize_with = "float_or_vec",default="default_zero")]
    pub explode: Vec<f32>,
    #[cfg(feature = "bigshield")]
    #[serde(default="default_false_vec", deserialize_with = "bool_or_vec")]
    pub bigshield: Vec<bool>,
    #[serde(default="default_bounce", deserialize_with = "float_or_vec")]
    pub bounceperc: Vec<f32>,
    #[cfg(feature = "stun")]
    #[serde(default="default_zero", deserialize_with = "float_or_vec")]//option:stun
    pub stun: Vec<f32>,//option:stun
    #[cfg(feature = "shrapnel")]
    #[serde(default="default_shrapnel", deserialize_with = "usize_or_vec")]//option:shrapnel
    pub shrapnel_amount:Vec<usize>,//option:shrapnel
    #[cfg(feature = "shrapnel")]
    #[serde(default="default_zero", deserialize_with = "float_or_vec")] 
    pub shrapnel_factor:Vec<f32>,//option:shrapnel
    #[cfg(feature = "escape")]
    #[serde(default="default_zero", deserialize_with = "float_or_vec")]
    pub escape_factor:Vec<f32>,
    #[cfg(feature = "escape")]
    #[serde(default="default_zero", deserialize_with = "float_or_vec")]
    pub escape_threshold:Vec<f32>,
}


pub fn parse_battle(json_str: &str) -> Result<ParseBattleData, serde_json::Error> {
    serde_json::from_str(json_str)
}

#[wasm_bindgen]
pub fn wasm_fight_battle_rounds(input_json: &str) -> String {
    do_battle(input_json,false)
}

// the ffi interface
#[unsafe(no_mangle)]
pub extern "C" fn fight_battle_rounds(input_json: *const c_char)-> *mut c_char {  // 
    let input_str = unsafe { CStr::from_ptr(input_json).to_str().unwrap() };
    let battle_output = do_battle(input_str,true);
    let c_str = CString::new(battle_output).unwrap();
    c_str.into_raw()
}


fn get_shipinfo(ship: &ParseShipInfo,rfs: Vec<Vec<f32>>,fleet_idx: usize,map_id_to_index: &HashMap<usize, usize>) -> ShipInfo {
   ShipInfo {
				attack: ship.attack[0],
				shield_max: ship.shield[0],
                shield_bounce: ship.shield[0]*ship.bounceperc[0],
				hull_max: ship.hull,
                #[cfg(feature = "explode")]
                explode_trigger: ship.hull*ship.explode[0],
                rapidfire: rfs[0].clone(),
				player_id: fleet_idx,
                ship_id: map_id_to_index[&ship.shipid],
                #[cfg( feature = "perround")]
                round_rapidfire: rfs.clone(),
                #[cfg(feature = "perround")]
                round_attack: ship.attack.clone(),
                #[cfg(feature = "perround")]
                round_shield: ship.shield.clone(),   
                #[cfg(all(feature = "explode", feature = "perround"))]             
                round_explode: ship.explode.clone(),
                #[cfg(all(feature = "bigshield", feature = "perround"))]      
                round_bigshield: ship.bigshield.clone(),
                #[cfg(feature = "perround")]
                round_bounce: ship.bounceperc.clone(),
                #[cfg(all(feature = "stun", feature = "perround"))] 
                round_stun: ship.stun.clone(),
                #[cfg(feature = "stun")]
                stun: ship.stun[0],
                #[cfg(feature = "bigshield")]
                bigshield: ship.bigshield[0],
                shield_perc_bounce: ship.bounceperc[0],
                #[cfg(feature = "shrapnel")]
                shrapnelamt: ship.shrapnel_amount[0],
                #[cfg(feature = "shrapnel")]
                shrapnelfactor: ship.shrapnel_factor[0],
                #[cfg(all(feature = "shrapnel", feature = "perround"))]
                round_shrapnel: ship.shrapnel_amount.clone(),
                #[cfg(all(feature = "shrapnel", feature = "perround"))]
                round_shrapnelfactor: ship.shrapnel_factor.clone(),
                #[cfg(feature = "escape")]
                escape_factor: ship.escape_factor[0],
                #[cfg(all(feature = "escape", feature = "perround"))] 
                round_escape_factor: ship.escape_factor.clone(),   
                #[cfg(feature = "escape")]
                escape_threshold: ship.escape_threshold[0]* ship.hull,
                #[cfg(all(feature = "escape", feature = "perround"))] 
                round_escape_threshold: ship.escape_threshold.clone(), 
			}
}





// main function that executes the battle, first create all the ships with a index mapping, then calls the fight
pub fn do_battle(json: &str, is_local: bool) -> String {
    let start: Option<Instant> = if is_local {
        Some(Instant::now())
    } else {
        None
    };
    let parsed: ParseBattleData = serde_json::from_str(json).unwrap();
    let mut map_id_to_index: HashMap<usize, usize> = HashMap::new();
    let mut map_index_to_id: HashMap<usize, usize> = HashMap::new();
    let mut attacker_index_to_id: HashMap<usize, usize> = HashMap::new();
    let mut defender_index_to_id: HashMap<usize, usize> = HashMap::new();
    let mut attackers: Vec<Player> = Vec::new();
    let mut attacker_amount: Vec<Vec<usize>> = Vec::new();
    let mut defenders: Vec<Player> = Vec::new();
    let mut defender_amount: Vec<Vec<usize>> = Vec::new();
    for fleet in &parsed.attacker {
        for ship in &fleet.ships {
            if !map_id_to_index.contains_key(&ship.shipid) {
            map_id_to_index.insert(ship.shipid, map_id_to_index.len());
            }
        }
    }    
    for fleet in &parsed.defender {
        for ship in &fleet.ships {
            if !map_id_to_index.contains_key(&ship.shipid) {
            map_id_to_index.insert(ship.shipid, map_id_to_index.len());
            }
        }
    }    
    for (shipid, index) in &map_id_to_index {
        map_index_to_id.insert(*index, *shipid);
    }


    for  (fleet_idx, fleet) in parsed.attacker.iter().enumerate(){
        let mut shipinfos: Vec<ShipInfo> = Vec::new();
        let mut ship_counts: Vec<usize> = Vec::new();
        for ship in &fleet.ships {
            ship_counts.push(ship.amount);
            let rfs=map_rapidfire_vec_to_idx(&ship.rapidfire,&map_id_to_index);
            shipinfos.push(get_shipinfo(ship,rfs,fleet_idx,&map_id_to_index));
        }
        attacker_amount.push(ship_counts);
        attacker_index_to_id.insert(fleet_idx, fleet.name);
        attackers.push(Player {
            player_id: fleet_idx,
            ships: shipinfos,
        });
    }    

    for  (fleet_idx, fleet) in parsed.defender.iter().enumerate(){
        let mut shipinfos: Vec<ShipInfo> = Vec::new();
        let mut ship_counts: Vec<usize> = Vec::new();
        for ship in &fleet.ships {
            ship_counts.push(ship.amount);
            let rfs: Vec<Vec<f32>>=map_rapidfire_vec_to_idx(&ship.rapidfire,&map_id_to_index);
            shipinfos.push(get_shipinfo(ship,rfs,fleet_idx,&map_id_to_index));
        }
        defender_amount.push(ship_counts);
        defender_index_to_id.insert(fleet_idx, fleet.name);
        defenders.push(Player {
            player_id: fleet_idx,
            ships: shipinfos,
        });
    }    
    
    let (mut ships_a, mut ship_a_infos) = create_all_ships(&attackers, &attacker_amount);
    let (mut ships_b, mut ship_b_infos) = create_all_ships(&defenders, &defender_amount);
    let num_players_a=attackers.len();
    let num_players_b=defenders.len();

    if let Some(start_time) = start {
    let elapsed = start_time.elapsed();
    println!("Elapsed time: {:} ms for init of ships", elapsed.as_millis());
}

    let rt=execute_fight(parsed.rounds, &mut ships_a,&mut ships_b,
        &mut ship_a_infos,&mut ship_b_infos,&map_id_to_index,num_players_a,
        num_players_b,&attacker_index_to_id,&defender_index_to_id,&map_index_to_id,is_local);
    if let Some(start_time) = start {
    let elapsed = start_time.elapsed();
    println!("Elapsed time: {:} ms for the whole fight", elapsed.as_millis());
}
    rt
}

//creates the ships for the player
pub fn create_all_ships(
    players: &[Player],
    ship_counts: &[Vec<usize>]
) -> (Vec<Ship>,Vec<ShipInfo>) {
    let mut all_ships = Vec::with_capacity(ship_counts.iter().flatten().sum());
    let mut all_ship_infos = Vec::with_capacity(ship_counts.iter().map(Vec::len).sum());
	for (player_idx, player) in players.iter().enumerate() {
		let counts = &ship_counts[player_idx];
		for (shiptype, &count) in counts.iter().enumerate() {

			for _ in 0..count {
				all_ships.push(Ship {
					hull: ThreadCell(UnsafeCell::new(player.ships[shiptype].hull_max)),
					shield: ThreadCell(UnsafeCell::new(player.ships[shiptype].shield_max)),
					info: all_ship_infos.len() as u16,
                    #[cfg(feature = "flags_self")]
                    flagsself: ThreadCell(UnsafeCell::new(0)),
                    #[cfg(feature = "flags_other")]
                    flagsother: ThreadCell(UnsafeCell::new(0)),
				});
                
			}
            all_ship_infos.push(player.ships[shiptype].clone());
		}
	}
	(all_ships, all_ship_infos)
}


fn map_rapidfire_vec_to_idx(    rapidfire: &Vec<HashMap<usize, f32>>,
    index_mapping: &HashMap<usize, usize>,
) -> Vec<Vec<f32>>{
    let mut vret: Vec<Vec<f32>>=Vec::with_capacity(rapidfire.len());
    for rf in rapidfire.iter(){
        vret.push(map_rapidfire_to_vector(rf,index_mapping));
    }
    vret
}

fn map_rapidfire_to_vector(
    rapidfire: &HashMap<usize, f32>,
    index_mapping: &HashMap<usize, usize>,
) -> Vec<f32> {
    let mut result = vec![0.0_f32; index_mapping.len()];
    for (&key, &value) in rapidfire {
        if let Some(&index) = index_mapping.get(&key) {
                result[index] = transform(value);
        }
    }
    result
}

fn transform(n: f32) -> f32 {
    let mut x=0.0;
    if n != 0.0 { 
         x = (n-1.0)/n;
        }
    x
}


fn execute_fight(rounds:usize,fleet_a:  &mut Vec<Ship>,fleet_b: &mut Vec<Ship>,ship_a_infos:&mut Vec<ShipInfo>,ship_b_infos: &mut Vec<ShipInfo>,map_id_to_index:&HashMap<usize, usize>,player_amt_a:usize,player_amt_b:usize,attacker_index_to_id: &HashMap<usize, usize>,defender_index_to_id: &HashMap<usize, usize>,map_index_to_id: &HashMap<usize, usize>,is_local: bool)-> String {
    let mut  roundstat_attacker :Vec<RoundstatsInternal> = Vec::new();
    let mut  roundstat_defender :Vec<RoundstatsInternal> = Vec::new();
    let mut statistics_a_vec: Vec<Statistics> = Vec::new();
    let mut statistics_b_vec: Vec<Statistics> = Vec::new();

    let mut round_stats_player_a=RoundstatsInternal::new(player_amt_a,map_id_to_index.len());
    let mut round_stats_player_b=RoundstatsInternal::new(player_amt_b,map_id_to_index.len());
    process_ships_after_round( fleet_a, &mut round_stats_player_a, ship_a_infos);
    process_ships_after_round( fleet_b, &mut  round_stats_player_b, ship_b_infos);
    roundstat_attacker.push(round_stats_player_a);
    roundstat_defender.push(round_stats_player_b);

    for _r in 0..rounds{
        println!("Round {}: Fleet A size: {}, Fleet B size: {}", _r, fleet_a.len(), fleet_b.len());
        if fleet_a.len()==0 || fleet_b.len()==0{
            break;
        }
        #[cfg(feature = "perround")]
        for ship_info in ship_a_infos.iter_mut() {
             apply_round_values(ship_info, _r);
        }
        #[cfg(feature = "perround")]
        for ship_info in ship_b_infos.iter_mut() {
             apply_round_values(ship_info, _r);
        }
        let mut statistics_a = Statistics::new(player_amt_a,player_amt_b,map_id_to_index.len());
        let mut statistics_b = Statistics::new(player_amt_b,player_amt_a,map_id_to_index.len());
        multishoot( &fleet_a,   &fleet_b, &mut statistics_a, &mut statistics_b,ship_a_infos,ship_b_infos,is_local);
        let mut round_stats_player_a=RoundstatsInternal::new(player_amt_a,map_id_to_index.len());
        let mut round_stats_player_b=RoundstatsInternal::new(player_amt_b,map_id_to_index.len());
        process_ships_after_round( fleet_a, &mut round_stats_player_a, ship_a_infos);
        process_ships_after_round( fleet_b, &mut  round_stats_player_b, ship_b_infos);
        roundstat_attacker.push(round_stats_player_a);
        roundstat_defender.push(round_stats_player_b);
        statistics_a_vec.push(statistics_a);
        statistics_b_vec.push(statistics_b);
   
    }
    stats_update_lost(&mut roundstat_attacker);
    stats_update_lost(&mut roundstat_defender);

    println!("Final Fleet A size: {}, Final Fleet B size: {}", fleet_a.len(), fleet_b.len());
    let mut outcome=0;
    if fleet_a.len()==0 && fleet_b.len()>0{
        outcome=-1;
    }
    if fleet_a.len()>0 && fleet_b.len()==0{
        outcome=1;
    }

    //generate all the output...
    let mut rt=RootStats{
        rounds: Vec::new(),
        outcome: outcome,
    };
    let empty_statistics_a = Statistics::new(player_amt_a, player_amt_b, map_id_to_index.len());
    let empty_statistics_b = Statistics::new(player_amt_b, player_amt_a, map_id_to_index.len());
    rt.rounds.push(RoundStats {
        attacker: get_stats_object(
            &empty_statistics_a,
            roundstat_attacker[0].clone(),
            attacker_index_to_id,
            defender_index_to_id,
            map_index_to_id,
        ),
        defender: get_stats_object(
            &empty_statistics_b,
            roundstat_defender[0].clone(),
            defender_index_to_id,
            attacker_index_to_id,
            map_index_to_id,
        ),
    });


    for r in 0..statistics_a_vec.len(){
        let attackersach=get_stats_object(&statistics_a_vec[r],roundstat_attacker[r+1].clone(),attacker_index_to_id,defender_index_to_id,map_index_to_id);
        let defendersach=get_stats_object(&statistics_b_vec[r],roundstat_defender[r+1].clone(),defender_index_to_id,attacker_index_to_id,map_index_to_id);

        let roundstat=RoundStats{
            attacker: attackersach,
            defender: defendersach,
        };
        rt.rounds.push(roundstat);
    }

    
    serde_json::to_string(&rt).unwrap()
    
}

fn stats_update_lost(roundstat: &mut Vec<RoundstatsInternal>) {

    for i in 1..roundstat.len() {
        for p in 0..roundstat[i].stats.len() {
            for s in 0..roundstat[i].stats[p].len() {

                let prev_amount = roundstat[i - 1].stats[p][s].amount;
                let curr_amount = roundstat[i].stats[p][s].amount;
                #[cfg(not(feature = "escape"))]
                let lost = prev_amount - curr_amount;
                #[cfg(feature = "escape")]
                let lost = prev_amount - curr_amount - roundstat[i].stats[p][s].escape;
                if lost > 0 {
                    roundstat[i].stats[p][s].lost = lost as usize;
                }
            }
        }
    }
    
}

pub fn process_ships_after_round(ships:  &mut Vec<Ship>, round_stat: &mut RoundstatsInternal, ship_infos: &Vec<ShipInfo>) {
    ships.retain(|ship| ship.hull.get() != 0.0);
    for ship in ships.iter_mut() {
        ship.shield.set(ship_infos[ship.info as usize].shield_max);  

        #[cfg(feature = "escape")]      
        if get_flag_other(ship, OTHER_FLAG_ESCAPE){
            round_stat.stats[ship_infos[ship.info as usize].player_id][ship_infos[ship.info as usize].ship_id].escape += 1;
            continue;
        }
        #[cfg(feature = "stun")]      
        if get_flag_other(ship, OTHER_FLAG_STUNNING){
            set_flag_self(ship, SELF_FLAG_STUNNED);
            unset_flag_other(ship, OTHER_FLAG_STUNNING);
            round_stat.stats[ship_infos[ship.info as usize].player_id][ship_infos[ship.info as usize].ship_id].stunned += 1;
        }else{
        round_stat.stats[ship_infos[ship.info as usize].player_id][ship_infos[ship.info as usize].ship_id].attack += ship_infos[ship.info as usize].attack as i64;
        }

        #[cfg(not(feature = "stun"))]
        {
        round_stat.stats[ship_infos[ship.info as usize].player_id][ship_infos[ship.info as usize].ship_id].attack += ship_infos[ship.info as usize].attack as i64;
        }
        round_stat.stats[ship_infos[ship.info as usize].player_id][ship_infos[ship.info as usize].ship_id].shield += ship.shield.get()  as i64;
        round_stat.stats[ship_infos[ship.info as usize].player_id][ship_infos[ship.info as usize].ship_id].hull += ship.hull.get() as i64;
        round_stat.stats[ship_infos[ship.info as usize].player_id][ship_infos[ship.info as usize].ship_id].amount += 1;
    }
    #[cfg(feature = "escape")] 
    ships.retain(|ship|get_flag_other(ship, OTHER_FLAG_ESCAPE)==false);
}
#[cfg(feature = "perround")]
pub fn apply_round_values(ship: &mut ShipInfo, index: usize) {
    if let Some(&v) = ship.round_attack.get(index) {
        ship.attack = v;
    }
    if let Some(&v) = ship.round_shield.get(index) {
        ship.shield_max = v;
    }
    #[cfg(feature = "explode")]
    if let Some(&v) = ship.round_explode.get(index) {
        ship.explode_trigger = v * ship.hull_max;
    }
    #[cfg(feature = "bigshield")]
    if let Some(&v) = ship.round_bigshield.get(index) {
        ship.bigshield = v;
    }
    if let Some(&v) = ship.round_bounce.get(index) {
        ship.shield_perc_bounce = v;
    }
    if let Some(v) = ship.round_rapidfire.get(index) {
        ship.rapidfire = v.clone();
    }
    #[cfg(feature = "stun")]
    if let Some(&v) = ship.round_stun.get(index) {
        ship.stun = v;
    }
    #[cfg(feature = "shrapnel")]
    if let Some(&v) = ship.round_shrapnel.get(index) {
        ship.shrapnelamt = v;
    }
    #[cfg(feature = "shrapnel")]
    if let Some(&v) = ship.round_shrapnelfactor.get(index) {
        ship.shrapnelfactor = v;
    }
    #[cfg(feature = "escape")]
    if let Some(&v) = ship.round_escape_threshold.get(index) {
        ship.escape_threshold = v* ship.hull_max;
    }
    #[cfg(feature = "escape")]
    if let Some(&v) = ship.round_escape_factor.get(index) {
        ship.escape_factor = v;
    }

    ship.shield_bounce = ship.shield_max*ship.shield_perc_bounce;
    
}


fn multishoot(slicea: &[Ship],sliceb: &[Ship],statistic_a: &mut Statistics,statistics_b: &mut Statistics,ship_a_infos: &Vec<ShipInfo>,ship_b_infos: &Vec<ShipInfo>,is_local: bool) {
if is_local {

    thread::scope(|s| {
    s.spawn(|_| {
        shoot(slicea, sliceb, statistic_a, ship_a_infos, ship_b_infos);
    });
    s.spawn(|_| {
        shoot(sliceb, slicea, statistics_b, ship_b_infos, ship_a_infos);
    });
})
.unwrap();

}else{
    shoot(slicea, sliceb, statistic_a, ship_a_infos, ship_b_infos);
    shoot(sliceb, slicea, statistics_b, ship_b_infos, ship_a_infos);
}

}


pub fn shoot(attacker_ships: & [Ship], defender_ships: & [Ship], statistics: &mut Statistics,ship_a_infos: &Vec<ShipInfo>,ship_b_infos: &Vec<ShipInfo>) {
	let mut rng = rand::rng();
    #[cfg(feature = "bigshield")]
    let  mut shipshields=filter_ships_shields(defender_ships, ship_b_infos);
    #[cfg(not(feature = "bigshield"))]
    let  mut shipshields=Vec::new();
    let total_ships=defender_ships.len();
    let mut killed_ships=0usize;
	for attacker in attacker_ships.iter() {
		shoot_once(attacker, &ship_a_infos[attacker.info as usize], defender_ships, ship_b_infos, statistics, &mut rng, &mut shipshields,total_ships,&mut killed_ships);
	}
}

#[cfg(feature = "bigshield")]
fn filter_ships_shields<'a>(ships: &'a [Ship], ship_infos: &'a [ShipInfo]) -> Vec<&'a Ship> {
    ships
        .iter()
        .filter(|ship| ship_infos[ship.info as usize].bigshield)
        .collect()
}

#[cfg(feature = "stun")]
fn do_stun<'a, R: rand::Rng + ?Sized>(target: &Ship, target_info: &ShipInfo, statistics: &mut Statistics, attacker_info: &ShipInfo, rng: &mut R,attack : &f32) {
        if attacker_info.stun == 0.0 || target.hull.get() == 0.0 {
            return; // no stun capability, skip
        }

        if rng.random::<f32>() < attacker_info.stun*(attack / target_info.hull_max) {
            set_flag_other(target, OTHER_FLAG_STUNNING);//TODO: add stun to statistics!!
            statistics.stunned_done[attacker_info.player_id][target_info.player_id][attacker_info.ship_id][target_info.ship_id] += 1;
        }
}



pub fn shoot_once<'a, R: rand::Rng + ?Sized>(_attacker: & Ship,attacker_info: &ShipInfo, defender_ships: &'a [Ship], defender_infos: &Vec<ShipInfo>, _statistics: &mut Statistics, rng: &mut R, _shipshields: &mut Vec<&'a Ship>,_total_ships:usize,_killed_ships: &mut usize){
	#[cfg(feature = "stun")]
    if get_flag_self(_attacker, SELF_FLAG_STUNNED){ //is stunned
        unset_flag_self(_attacker, SELF_FLAG_STUNNED);
        return;
    }
    loop {   
    #[cfg(feature = "bigshield")]
    let target: &Ship = pick_random_ship( _shipshields, defender_ships, rng).unwrap();
    #[cfg(not(feature = "bigshield"))]
    let target: &Ship = defender_ships.choose(rng).unwrap();
    let target_info = &defender_infos[target.info as usize];
	// Check if attack is higher than target's shield_bounce
	if attacker_info.attack > target_info.shield_bounce || target.shield.get() == 0.0 {
		// Penetration: attack minus shield
		if target.hull.get() == 0.0 {
            _statistics.damage_dead[attacker_info.player_id][target_info.player_id][attacker_info.ship_id][target_info.ship_id] += attacker_info.attack as f64;
        }else{
            if target.shield.get() == 0.0 {
                let damage_done = attacker_info.attack.min( target.hull.get());
                let overflow = attacker_info.attack - damage_done;
                if overflow>0.0{
                    _statistics.damage_dead[attacker_info.player_id][target_info.player_id][attacker_info.ship_id][target_info.ship_id] += overflow as f64;
                }
                _statistics.damage_done[attacker_info.player_id][target_info.player_id][attacker_info.ship_id][target_info.ship_id] += damage_done as f64;
                target.hull.set(target.hull.get() - damage_done);
                // check for stun!
                    #[cfg(feature = "stun")]
                    do_stun(target, target_info, _statistics, attacker_info, rng, &attacker_info.attack);

            }else{
                let penetration = attacker_info.attack-target.shield.get();
                if penetration >0.0 {
                    // Apply damage to hull
                    let damage_done = penetration.min(target.hull.get());
                    let overflow = penetration - damage_done;
                    if overflow>0.0{
                        _statistics.damage_dead[attacker_info.player_id][target_info.player_id][attacker_info.ship_id][target_info.ship_id] += overflow as f64;
                    }
                    _statistics.damage_done[attacker_info.player_id][target_info.player_id][attacker_info.ship_id][target_info.ship_id] += damage_done as f64;
                    _statistics.shield_hit[attacker_info.player_id][target_info.player_id][attacker_info.ship_id][target_info.ship_id] += target.shield.get() as f64;
                    target.shield.set(0.0);
                    target.hull.set(target.hull.get()-damage_done);
                    // check for stun!
                    #[cfg(feature = "stun")]
                    do_stun(target, target_info, _statistics, attacker_info, rng, &attacker_info.attack);
                }else{
                    _statistics.shield_hit[attacker_info.player_id][target_info.player_id][attacker_info.ship_id][target_info.ship_id] += attacker_info.attack as f64;
                    target.shield.set(target.shield.get() - attacker_info.attack);
                }
            }
            if target.hull.get() == 0.0 {
                _statistics.ship_destroyed[attacker_info.player_id][target_info.player_id][attacker_info.ship_id][target_info.ship_id] += 1;
                   #[cfg(feature = "rfcancel")]
                    {
                        *_killed_ships += 1;
                    }

            }
            #[cfg(feature = "explode")]
			if target.hull.get() < target_info.explode_trigger && target.hull.get() > 0.0 {
				if rng.random::<f32>() > target.hull.get() / target_info.hull_max {
                    _statistics.explosion_damage_done[attacker_info.player_id][target_info.player_id][attacker_info.ship_id][target_info.ship_id] += target.hull.get() as f64;
                    target.hull.set( 0.0);
                    _statistics.explosion_triggered[attacker_info.player_id][target_info.player_id][attacker_info.ship_id][target_info.ship_id] += 1;
                   #[cfg(feature = "rfcancel")]
                    {
                        *_killed_ships += 1;
                    }
                }
            }

        }

	} else {
		_statistics.shield_bounced[attacker_info.player_id][target_info.player_id][attacker_info.ship_id][target_info.ship_id] += attacker_info.attack as f64;
	}


    #[cfg(feature = "shrapnel")]
    if attacker_info.shrapnelamt > 0 {
        for _ in 0..attacker_info.shrapnelamt {
            shoot_once_with_settings(attacker_info, defender_ships, defender_infos, _statistics, rng,  _shipshields, attacker_info.shrapnelfactor*attacker_info.attack,_total_ships,_killed_ships);
        }
    }
    #[cfg(feature = "escape")]
    escape_check(target, target_info, _statistics, attacker_info, rng);

    	// Rapidfire: continue the loop instead of recursing.
	if attacker_info.rapidfire[target_info.ship_id] > 0.0
		&& rng.random::<f32>() < attacker_info.rapidfire[target_info.ship_id] {
        #[cfg(feature = "rfcancel")]
        if *_killed_ships == _total_ships {
            _statistics.rf_stopped[attacker_info.player_id][attacker_info.ship_id] += 1;
            break
        }
		_statistics.rapid_fire_done[attacker_info.player_id][target_info.player_id][attacker_info.ship_id][target_info.ship_id] += 1;
		continue;
	}
	break;
    } // end loop
}

#[cfg(feature = "escape")]
fn escape_check<'a, R: rand::Rng + ?Sized>(target: &Ship, target_info: &ShipInfo, _statistics: &mut Statistics, attacker_info: &ShipInfo, rng: &mut R) {
    if target_info.escape_threshold > target.hull.get() && target.hull.get() > 0.0 {
        if !get_flag_other(target,OTHER_FLAG_ESCAPE){
            if rng.random::<f32>() < target_info.escape_factor*(target.hull.get() / target_info.hull_max) {
                set_flag_other(target, OTHER_FLAG_ESCAPE);
                _statistics.escape_triggered[attacker_info.player_id][target_info.player_id][attacker_info.ship_id][target_info.ship_id] += 1;
            }
          }
    }
}




pub fn shoot_once_with_settings<'a, R: rand::Rng + ?Sized>(attacker_info: &ShipInfo, 
    defender_ships: &'a [Ship], defender_infos: &Vec<ShipInfo>, _statistics: &mut Statistics, 
    rng: &mut R, _shipshields: &mut Vec<&'a Ship>,shipattack: f32,_total_ships:usize,_killed_ships: &mut usize) {

    #[cfg(feature = "bigshield")]
    let target: &Ship = pick_random_ship( _shipshields, defender_ships, rng).unwrap();
    #[cfg(not(feature = "bigshield"))]
    let target: &Ship = defender_ships.choose(rng).unwrap();

    let target_info = &defender_infos[target.info as usize];
	// Check if attack is higher than target's shield_bounce
	if shipattack > target_info.shield_bounce || target.shield.get() == 0.0 {
		// Penetration: attack minus shield
		if target.hull.get() == 0.0 {
            _statistics.damage_dead[attacker_info.player_id][target_info.player_id][attacker_info.ship_id][target_info.ship_id] += attacker_info.attack as f64;
        }else{
            if target.shield.get() == 0.0 {
                let damage_done = shipattack.min( target.hull.get());
                let overflow = attacker_info.attack - damage_done;
                if overflow>0.0{
                    _statistics.damage_dead[attacker_info.player_id][target_info.player_id][attacker_info.ship_id][target_info.ship_id] += overflow as f64;
                }
                _statistics.damage_done[attacker_info.player_id][target_info.player_id][attacker_info.ship_id][target_info.ship_id] += damage_done as f64;
                target.hull.set(target.hull.get() - damage_done);
                // check for stun!
                    #[cfg(feature = "stun")]
                    do_stun(target, target_info, _statistics, attacker_info, rng, &shipattack);

            }else{
                let penetration = shipattack-target.shield.get();
                if penetration >0.0 {
                    // Apply damage to hull
                    let damage_done = penetration.min(target.hull.get());
                    let overflow = penetration - damage_done;
                    if overflow>0.0{
                        _statistics.damage_dead[attacker_info.player_id][target_info.player_id][attacker_info.ship_id][target_info.ship_id] += overflow as f64;
                    }
                    _statistics.damage_done[attacker_info.player_id][target_info.player_id][attacker_info.ship_id][target_info.ship_id] += damage_done as f64;
                    _statistics.shield_hit[attacker_info.player_id][target_info.player_id][attacker_info.ship_id][target_info.ship_id] += target.shield.get() as f64;
                    target.shield.set(0.0);
                    target.hull.set(target.hull.get()-damage_done);
                    // check for stun!
                    #[cfg(feature = "stun")]
                    do_stun(target, target_info, _statistics, attacker_info, rng, &shipattack);
                    
                }else{
                    _statistics.shield_hit[attacker_info.player_id][target_info.player_id][attacker_info.ship_id][target_info.ship_id] += attacker_info.attack as f64;
                    target.shield.set(target.shield.get() - attacker_info.attack);
                }
            }
            if target.hull.get() == 0.0 {
                _statistics.ship_destroyed[attacker_info.player_id][target_info.player_id][attacker_info.ship_id][target_info.ship_id] += 1;
                    #[cfg(feature = "rfcancel")]
                    {
                        *_killed_ships += 1;
                    }
            }
            #[cfg(feature = "explode")]
			if target.hull.get() < target_info.explode_trigger && target.hull.get() > 0.0 {
				if rng.random::<f32>() > target.hull.get() / target_info.hull_max {
                    _statistics.explosion_damage_done[attacker_info.player_id][target_info.player_id][attacker_info.ship_id][target_info.ship_id] += target.hull.get() as f64;
                    target.hull.set( 0.0);
                    _statistics.explosion_triggered[attacker_info.player_id][target_info.player_id][attacker_info.ship_id][target_info.ship_id] += 1;
                    #[cfg(feature = "rfcancel")]
                    {
                        *_killed_ships += 1;
                    }
                }
            }

        }

	} else {
		_statistics.shield_bounced[attacker_info.player_id][target_info.player_id][attacker_info.ship_id][target_info.ship_id] += shipattack as f64;
	}
    #[cfg(feature = "escape")]
    escape_check(target, target_info, _statistics, attacker_info, rng);
}

#[cfg(feature = "bigshield")]
fn pick_random_ship<'a, R: rand::Rng + ?Sized>(
    ships: &mut Vec<&'a Ship>,
    fallback: &'a [Ship],
    rng: &mut R,
) -> Option<&'a Ship> {
    
    while !ships.is_empty() {
        let idx = rng.random_range(0..ships.len());

        if ships[idx].shield.get() == 0.0 {
            ships.swap_remove(idx); 
            continue;
        }
        return Some(ships[idx]);
    }
    // fallback
    return fallback.choose(rng)
     
}


pub struct Player {
	pub player_id: usize,
	pub ships: Vec<ShipInfo>,
}



#[derive(Clone)]
pub struct ShipInfo {
	pub attack: f32,
	pub shield_max: f32,
    pub shield_bounce: f32,
	pub hull_max: f32,
    #[cfg(feature = "explode")]
    pub explode_trigger: f32,
    pub rapidfire: Vec<f32>,
	pub player_id: usize,
    pub ship_id: usize,
    #[cfg(feature = "perround")]
    pub round_attack: Vec<f32>,
    #[cfg(feature = "perround")]
    pub round_shield: Vec<f32>,
    #[cfg(all(feature = "explode", feature = "perround"))]
    pub round_explode: Vec<f32>,
    #[cfg(all(feature = "bigshield", feature = "perround"))]
    pub round_bigshield: Vec<bool>,
    #[cfg(feature = "perround")]
    pub round_bounce: Vec<f32>,
    #[cfg( feature = "perround")]
    pub round_rapidfire: Vec<Vec<f32>>,
    #[cfg(all(feature = "shrapnel", feature = "perround"))]
    pub round_shrapnel: Vec<usize>,
    #[cfg(all(feature = "shrapnel", feature = "perround"))]
    pub round_shrapnelfactor: Vec<f32>,
    #[cfg( feature = "bigshield")]
    pub bigshield: bool,
    pub shield_perc_bounce: f32,
    #[cfg(feature = "stun")]
    pub stun: f32,
    #[cfg(all(feature = "stun", feature = "perround"))]
    pub round_stun: Vec<f32>,
    #[cfg(feature = "shrapnel")]
    pub shrapnelamt:usize,
    #[cfg(feature = "shrapnel")]
    pub shrapnelfactor:f32,
    #[cfg(feature = "escape")]
    pub escape_factor: f32,
    #[cfg(all(feature = "escape", feature = "perround"))]
    pub round_escape_factor: Vec<f32>,
    #[cfg(feature = "escape")]
    pub escape_threshold: f32,
    #[cfg(all(feature = "escape", feature = "perround"))]
    pub round_escape_threshold: Vec<f32>,
}
#[cfg(feature = "flags_other")]
fn get_flag_other(ship: &Ship, index: u8) -> bool {
    let flags = ship.flagsother.get();
    (flags & (1 << index)) != 0
}
#[cfg(feature = "flags_self")]
fn get_flag_self(ship: &Ship, index: u8) -> bool {
    let flags = ship.flagsself.get();
    (flags & (1 << index)) != 0
}
#[cfg(feature = "flags_self")]
fn set_flag_self(ship: &Ship, index: u8) {
    let mut flags = ship.flagsself.get();
    flags |= 1 << index;
    ship.flagsself.set(flags);
}
#[cfg(feature = "flags_other")]
fn set_flag_other(ship: &Ship, index: u8) {
    let mut flags = ship.flagsother.get();
    flags |= 1 << index;
    ship.flagsother.set(flags);
}
#[cfg(feature = "flags_self")]
fn unset_flag_self(ship: &Ship, index: u8) {
    let mut flags = ship.flagsself.get();
    flags &= !(1 << index);
    ship.flagsself.set(flags);
}
#[cfg(feature = "flags_other")]
fn unset_flag_other(ship: &Ship, index: u8) {
    let mut flags = ship.flagsother.get();
    flags &= !(1 << index);
    ship.flagsother.set(flags);
}

#[repr(C)]
pub struct Ship {
	pub hull: ThreadCell<f32>,
	pub shield: ThreadCell<f32>,
	pub info: u16,
    #[cfg(feature = "flags_self")]
    pub flagsself: ThreadCell<u8>,
    #[cfg(feature = "flags_other")]
    pub flagsother: ThreadCell<u8>,
}


pub struct Statistics {
	pub damage_done: Vec<Vec<Vec<Vec<f64>>>>,
    pub damage_dead: Vec<Vec<Vec<Vec<f64>>>>,
	pub shield_hit: Vec<Vec<Vec<Vec<f64>>>>,
	pub ship_destroyed: Vec<Vec<Vec<Vec<i64>>>>,
	pub shield_bounced: Vec<Vec<Vec<Vec<f64>>>>,
	pub rapid_fire_done: Vec<Vec<Vec<Vec<i64>>>>,
    #[cfg(feature = "explode")]
	pub explosion_triggered: Vec<Vec<Vec<Vec<i64>>>>,
    #[cfg(feature = "explode")]
    pub explosion_damage_done: Vec<Vec<Vec<Vec<f64>>>>,
    #[cfg(feature = "stun")]
    pub stunned_done: Vec<Vec<Vec<Vec<i64>>>>,
    #[cfg(feature = "escape")]
    pub escape_triggered: Vec<Vec<Vec<Vec<i64>>>>,
    #[cfg(feature = "rfcancel")]
    pub rf_stopped: Vec<Vec<i64>>
}



impl Statistics {
	pub fn new(player_amount_a: usize,player_amount_b: usize,ship_amount:usize) -> Self {
		let empty_matrix = || vec![vec![vec![vec![0.0; ship_amount];ship_amount]; player_amount_b]; player_amount_a];
        let empty_matrix_int = || vec![vec![vec![vec![0; ship_amount];ship_amount]; player_amount_b]; player_amount_a];
		Statistics {
			damage_done: empty_matrix(),
            damage_dead: empty_matrix(),
			shield_hit: empty_matrix(),
			ship_destroyed: empty_matrix_int(),
			shield_bounced: empty_matrix(),
			rapid_fire_done: empty_matrix_int(),
            #[cfg(feature = "explode")]
			explosion_triggered: empty_matrix_int(),
            #[cfg(feature = "explode")]
            explosion_damage_done: empty_matrix(),
            #[cfg(feature = "stun")]
            stunned_done: empty_matrix_int(),
            #[cfg(feature = "escape")]
            escape_triggered: empty_matrix_int(),
            #[cfg(feature = "rfcancel")]
            rf_stopped:vec![vec![0; ship_amount]; player_amount_a],
		}
	}
}




//end shooting logic rest here is to get back to json!

fn get_stats_object(statistics:&Statistics,roundinfo:RoundstatsInternal,player_index_to_id: &HashMap<usize, usize>,player_index_to_id_b: &HashMap<usize, usize>,map_index_to_id: &HashMap<usize, usize>)-> Battlestats {
    let  battlestats=Battlestats{
        roundstats: from_internal_to_external_roundstats(roundinfo, player_index_to_id, map_index_to_id),
        general_statistics: GeneralStatistics{
            damage_done: generate_stat_block_from_vec(&statistics.damage_done, player_index_to_id, player_index_to_id_b, map_index_to_id),
            damage_dead: generate_stat_block_from_vec(&statistics.damage_dead, player_index_to_id, player_index_to_id_b, map_index_to_id),
            shield_hit: generate_stat_block_from_vec(&statistics.shield_hit, player_index_to_id, player_index_to_id_b, map_index_to_id),
            ship_destroyed: generate_stat_block_from_vec(&statistics.ship_destroyed, player_index_to_id, player_index_to_id_b, map_index_to_id),
            shield_bounced: generate_stat_block_from_vec(&statistics.shield_bounced, player_index_to_id, player_index_to_id_b, map_index_to_id),
            rapid_fire_done: generate_stat_block_from_vec(&statistics.rapid_fire_done, player_index_to_id, player_index_to_id_b, map_index_to_id),
            #[cfg(feature = "explode")]
            explosion_triggered: generate_stat_block_from_vec(&statistics.explosion_triggered, player_index_to_id, player_index_to_id_b, map_index_to_id),
            #[cfg(feature = "explode")]
            explosion_damage_done: generate_stat_block_from_vec(&statistics.explosion_damage_done, player_index_to_id, player_index_to_id_b, map_index_to_id),
            #[cfg(feature = "stun")]
            stunned_done: generate_stat_block_from_vec(&statistics.stunned_done, player_index_to_id, player_index_to_id_b, map_index_to_id),
            #[cfg(feature = "rfcancel")]
            rf_stopped: generate_stat_block_from_vec_two(&statistics.rf_stopped, player_index_to_id, map_index_to_id),
            
        },
    };

    battlestats
}

fn from_internal_to_external_roundstats(internal: RoundstatsInternal,player_index_to_id: &HashMap<usize, usize>,map_index_to_id: &HashMap<usize, usize>)-> HashMap<usize, HashMap<usize, Shipstats>> {
    let mut external: HashMap<usize, HashMap<usize, Shipstats>> = HashMap::new();
    for (player_idx, shipstats_vec) in internal.stats.iter().enumerate() {
        let player_id = player_index_to_id.get(&player_idx).unwrap();
        
        let mut shipstats_map:  HashMap<usize, Shipstats> = HashMap::new();
        for (ship_idx, shipstats) in shipstats_vec.iter().enumerate() {
            let ship_id = map_index_to_id.get(&ship_idx).unwrap();
            shipstats_map.insert(*ship_id, shipstats.clone());
        }
        external.insert(*player_id, shipstats_map);
    }
    prune_shipstats_level1(&mut external);
    external
}

fn prune_level1(map: &mut HashMap<
    usize,
    HashMap<usize, HashMap<usize, HashMap<usize, i64>>>,
>) {
    map.retain(|_, inner| !prune_level2(inner));
}

fn prune_level2(map: &mut HashMap<usize, HashMap<usize, HashMap<usize, i64>>>) -> bool {
    map.retain(|_, inner| !prune_level3(inner));
    map.is_empty()
}

fn prune_level3(map: &mut HashMap<usize, HashMap<usize, i64>>) -> bool {
    map.retain(|_, inner| !prune_level4(inner));
    map.is_empty()
}

fn prune_level4(map: &mut HashMap<usize, i64>) -> bool {
    map.retain(|_, v| *v != 0);
    map.is_empty()
}


pub fn generate_stat_block_from_vec<T>(statistics_vec: &Vec<Vec<Vec<Vec<T>>>>,player_a_idx_to_id: &HashMap<usize, usize>,player_b_idx_to_id: &HashMap<usize, usize>,ship_idx_to_id: &HashMap<usize, usize>)-> StatBlock where
    T: ToPrimitive + Copy,{
    let mut stat_block = StatBlock {
        maps: HashMap::new(),
        sum:  0,
    };
    for (i, v1) in statistics_vec.iter().enumerate() {
        let i_key = player_a_idx_to_id.get(&i).unwrap();

        for (j, v2) in v1.iter().enumerate() {
            let j_key = player_b_idx_to_id.get(&j).unwrap();

            for (k, v3) in v2.iter().enumerate() {
                let k_key = ship_idx_to_id.get(&k).unwrap();

                for (l, value) in v3.iter().enumerate() {
                    let v = value.to_i64().unwrap_or(0);

                    stat_block
                        .maps
                        .entry(*i_key)
                        .or_insert_with(HashMap::new)
                        .entry(*j_key)
                        .or_insert_with(HashMap::new)
                        .entry(*k_key)
                        .or_insert_with(HashMap::new)
                        .insert(*ship_idx_to_id.get(&l).unwrap(), v);

                    stat_block.sum += v;
                }
            }
        }
    }
    prune_level1(&mut stat_block.maps);
    stat_block
}

fn prune_level1_2d(map: &mut HashMap<usize, HashMap<usize, i64>>) {
    map.retain(|_, inner| {
        inner.retain(|_, v| *v != 0);
        !inner.is_empty()
    });
}


pub fn generate_stat_block_from_vec_two<T>(
    statistics_vec: &Vec<Vec<T>>,
    player_a_idx_to_id: &HashMap<usize, usize>,
    ship_idx_to_id: &HashMap<usize, usize>,
) -> StatBlock2d
where
    T: ToPrimitive + Copy,
{
    let mut stat_block = StatBlock2d {
        maps: HashMap::new(),
        sum: 0,
    };

    for (i, v1) in statistics_vec.iter().enumerate() {
        let i_key = player_a_idx_to_id.get(&i).unwrap();

        for (j, value) in v1.iter().enumerate() {
            let v = value.to_i64().unwrap_or(0);

            stat_block
                .maps
                .entry(*i_key)
                .or_insert_with(HashMap::new)
                .insert(*ship_idx_to_id.get(&j).unwrap(), v);

            stat_block.sum += v;
        }
    }

    prune_level1_2d(&mut stat_block.maps);

    stat_block
}

#[derive(Serialize,Clone,Deserialize, Debug)]
pub struct Shipstats {
    pub attack: i64,
    pub shield: i64,
    pub hull: i64,
    pub amount: usize,
    pub lost: usize,
    #[cfg(feature= "stun")]
    pub stunned: usize,
    #[cfg(feature = "escape")]
    pub escape: usize,
}

impl Shipstats {
    pub fn is_zero(&self) -> bool {
        let base =
            self.amount == 0 &&
            self.lost == 0;

        #[cfg(feature = "escape")]
        {
            base && self.escape == 0
        }

        #[cfg(not(feature = "escape"))]
        {
            base
        }
    }
}

fn prune_shipstats_level2(map: &mut HashMap<usize, Shipstats>) -> bool {
    map.retain(|_, stats| !stats.is_zero());
    map.is_empty()
}

fn prune_shipstats_level1(map: &mut HashMap<usize, HashMap<usize, Shipstats>>) {
    map.retain(|_, inner| !prune_shipstats_level2(inner));
}


#[derive(Serialize, Deserialize, Debug)]
pub struct GeneralStatistics {
    pub damage_done: StatBlock,
    pub damage_dead: StatBlock,
    pub shield_hit: StatBlock,
    pub ship_destroyed: StatBlock,
    pub shield_bounced: StatBlock,
    #[cfg(feature = "stun")]
    pub stunned_done: StatBlock,
    pub rapid_fire_done: StatBlock,
    #[cfg(feature = "explode")]
    pub explosion_triggered: StatBlock,
    #[cfg(feature = "explode")]
    pub explosion_damage_done: StatBlock,
    #[cfg(feature = "rfcancel")]
    pub rf_stopped: StatBlock2d,

}
#[derive(Serialize, Deserialize, Debug)]
pub struct StatBlock{
maps:    HashMap<usize,                            // from_fleet_id
        HashMap<usize,                        // to_fleet_id
            HashMap<usize,                    // from_ship_id
                HashMap<usize, i64>           // to_ship_id → value
            >
        >
    >,
    sum: i64,

}
#[derive(Serialize, Deserialize, Debug)]
pub struct StatBlock2d{
maps:    HashMap<usize,   
        HashMap<usize, i64>         
    >,
    sum: i64,

}



#[derive(Serialize, Deserialize, Debug)]
pub struct Battlestats {
    #[serde(rename = "Roundstats")]
    pub roundstats: HashMap<usize, HashMap<usize, Shipstats>>,

    #[serde(rename = "Statistics")]
    pub general_statistics: GeneralStatistics,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RoundStats{
    pub attacker: Battlestats,
    pub defender: Battlestats,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RootStats {
    pub rounds: Vec<RoundStats>,
    pub outcome: i32,
}


#[derive(Clone)]
pub struct RoundstatsInternal{
	pub stats: Vec<Vec<Shipstats>>,
}

impl RoundstatsInternal {
	pub fn new(player_amount: usize,ship_amount: usize) -> Self {
		RoundstatsInternal {
			stats:         vec![
            vec![
                Shipstats {
                    attack: 0,
                    shield: 0,
                    hull: 0,
                    amount: 0,
                    lost: 0,
                    #[cfg(feature = "escape")]
                    escape: 0,
                    #[cfg(feature = "stun")]
                    stunned: 0,
                };
                ship_amount
            ];
            player_amount
        ]
		}
	}
}


