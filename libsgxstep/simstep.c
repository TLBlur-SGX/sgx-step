#include "simstep.h"

void start_single_stepping() { ENABLE_TF; }
void stop_single_stepping() { DISABLE_TF; }
