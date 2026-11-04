# Daily catchment and reservoir models

The catchment is an independently specified synthetic HBV-style fixture.
The pinned AWR source and its forcing were unavailable. Any eventual
comparison must match its update order, initial stores and parameter map.

Forcing is one row per consecutive day. Precipitation, potential
evapotranspiration and observations are depth in mm per day, temperature
is in degrees Celsius and area is in square kilometers. A missing
observation is JSON null. Discharge in cubic meters per second is
runoff depth times area times 1,000 divided by 86,400.

The parameter vector is soil capacity, recharge exponent, evaporation
threshold fraction, fast recession fraction, base recession fraction,
percolation limit and degree-day melt factor. Stores start empty. Snow
accumulates at or below zero degrees. Above zero, melt is limited by the
snow store. Liquid divides into recharge and soil input based on pre-step
soil saturation. Soil overflow joins the upper store, evaporation leaves
soil, percolation moves water to the lower store, and recession produces
runoff. The per-step balance is exposed. Warm-up forcing is simulated
before scoring. Training RMSE is the objective, and validation and test
metrics remain separate.

The reservoir runs in cubic meters with one record per day. The rule
decides discretionary release from previous storage. Realized inflow,
evaporation, demand withdrawal, discretionary release and spill are
applied in that order. Available outlet capacity and reserve constrain
both controlled outflows. The score is shortage plus 0.05 times spill.
Severity, shortage days and maximum consecutive shortage are retained.
