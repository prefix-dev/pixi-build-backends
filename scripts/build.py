from collections import defaultdict
import subprocess
import json
import re
import os

from generate_version_matrix import generate_matrix


def build():
    # get target argument from the command line
    import sys
    target = sys.argv[1]
    target_matrix = generate_matrix(target)

    # run pixi task to build every target
    for bin_dict in target_matrix:
        # os = target["os"]
        # target = target["target"]
        # set the env variable in python
        # echo "${{ matrix.bins.env_name }}=${{ matrix.bins.version }}" >> $GITHUB_ENV
        # print(f"::set-env name={target}::{os}")
        os.environ[bin_dict["env_name"]] = bin_dict["version"]

        print(f"Building {target} on {os}")
        subprocess.run(["pixi", "run", "build-recipe", "--recipe", f"recipe/{bin_dict["recipe_name"]}.yaml", f"--target-platform={target}"], check=True)



if __name__ == "__main__":
    build()
