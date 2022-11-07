![maintenance: actively developed](https://img.shields.io/badge/maintenance-actively--developed-brightgreen.svg)

# espb

Morrowind potions attributes balancing tool.

Example:

```shell
$ espb scan -p ru -o PotionsBalance.esp <path to openmw.cfg or Morrowind.ini>
$ espb init -t recommended -o PotionsBalance.csv
$ espb apply -p ru -s PotionsBalance.csv PotionsBalance.esp
```
