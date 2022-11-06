![maintenance: actively developed](https://img.shields.io/badge/maintenance-actively--developed-brightgreen.svg)

# potions-balance

Morrowind potions attributes balancing tool.

Example:

```shell
$ potions-balance scan -p ru -o PotionsBalance.esp <path to openmw.cfg or Morrowind.ini>
$ potions-balance init -t recommended -o PotionsBalance.csv
$ potions-balance apply -p ru -s PotionsBalance.csv PotionsBalance.esp
```
