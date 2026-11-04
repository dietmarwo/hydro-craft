import math
import random
import sys

from tools import good, run, save

binary = sys.argv[1]

def model(days):
    return {"kind":"catchment","area_km2":86.4,"forcing":days,"warmup":0,
            "train_end":len(days)//3,"validation_end":2*len(days)//3}
def day(i,p=0,t=5,pet=0,o=0):
    return {"day":i,"precipitation_mm":p,"temperature_c":t,"pet_mm":pet,"observed_mm":o}
truth=[10,1,1,0.5,0.3,2,3]
dry = good(run(binary,model([day(i) for i in range(6)]),[truth],trace=True)[0])
assert all(x["runoff_mm"] == 0 and max(x["stores_mm"]) == 0 for x in dry["trace"])
storm = model([day(0,20),day(1),day(2),day(3),day(4),day(5)])
m = good(run(binary,storm,[[10,1,1,0.5,0,0,0]],trace=True)[0])
assert [x["runoff_mm"] for x in m["trace"][:3]] == [5,2.5,1.25]
assert m["trace"][0]["discharge_m3_s"] == 5
snow = model([day(0,5,-2),day(1,2,0),day(2,0,1),day(3,0,3),day(4),day(5)])
s = good(run(binary,snow,[truth],trace=True)[0])
assert [r["stores_mm"][0] for r in s["trace"][:4]] == [5,7,4,0]
assert s["max_daily_balance_mm"] < 1e-12
def reference(days, p):
    # Independent scalar mass bookkeeping and reference runoff.
    cap, expo, lp, k1, k2, perc, degree = p
    snow, soil, fast, slow = 0.0, 0.0, 0.0, 0.0
    result=[]
    for d in days:
        precip,t=d["precipitation_mm"],d["temperature_c"]
        thaw=min(snow,degree*t) if t>0 else 0
        liquid=precip+thaw if t>0 else 0
        snow+=precip-liquid
        transfer=liquid*(soil/cap)**expo
        soil+=liquid-transfer
        fast+=transfer+max(soil-cap,0)
        soil=min(soil,cap)
        et=min(soil,d["pet_mm"]*min(1,soil/(lp*cap)))
        soil-=et
        through=min(fast,perc)
        slow+=through
        fast-=through
        runoff=fast*k1+slow*k2
        fast*=1-k1
        slow*=1-k2
        result.append(runoff)
    return result
rng=random.Random(123)
forcing=[day(i,rng.uniform(0,20),rng.uniform(-3,8),rng.uniform(0,2)) for i in range(90)]
observed=reference(forcing,truth)
for i,d in enumerate(forcing):
    d["observed_mm"]=None if i%11==0 else observed[i]
catch=model(forcing) | {"warmup":8}
candidates=[[10,1,1,0.5,i/20,2,3] for i in range(1,12)]
rows=run(binary,catch,candidates,trace=True)
winner=min(rows,key=lambda r:r["score"])
assert candidates[int(winner["id"])]==truth
metrics=good(winner)
error=max(abs(x-y["runoff_mm"]) for x,y in zip(observed,metrics["trace"],strict=True))
assert error < 1e-12 and metrics["test"]["rmse_mm"] < 1e-12
assert metrics["max_daily_balance_mm"] < 1e-10
res={"kind":"reservoir","capacity_m3":10,"initial_m3":5,"outlet_m3_per_day":4,"reserve_m3":2,
     "inputs":[{"inflow_m3":2,"evaporation_m3":1,"demand_m3":8},{"inflow_m3":20,"evaporation_m3":0,"demand_m3":0}]}
r=good(run(binary,res,[[1,0]],trace=True)[0])
assert r["trace"][0]["delivered_m3"]==4 and r["trace"][0]["storage_m3"]==2
assert r["trace"][1]["spill_m3"]==12 and r["shortage_m3"]==4 and r["max_balance_m3"]==0

def scenarios(seed):
    rand=random.Random(seed)
    return [res | {"capacity_m3":80,"initial_m3":30,"outlet_m3_per_day":25,
                   "inputs":[{"inflow_m3":rand.uniform(0,20),"evaporation_m3":1,"demand_m3":9} for _ in range(30)]}
            for _ in range(8)]
policies=[[t,g] for t in [.35,.6,.85] for g in [0,.5,1]]
def costs(seed):
    return list(zip(*(run(binary,s,policies) for s in scenarios(seed)),strict=True))
selection=costs(29)
policy=min(range(len(policies)),key=lambda i:sum(r["score"] for r in selection[i]))
hold=costs(31)[policy]
assert all(math.isfinite(r["score"]) for r in hold)
save({"model":catch,"parameters":truth}, {"truth":truth,"candidate_count":len(candidates),"max_reference_error_mm":error,
     "folds":{k:metrics[k] for k in ("train","validation","test")},"max_daily_balance_mm":metrics["max_daily_balance_mm"],
     "reservoir_policy":policies[policy],"reservoir_holdout":[r["metrics"] for r in hold],"analytic_tests":4})
