# SPARK-MIDEN-V1

### Settlement of Spot Order Trades with SWAPp

Settlement of spot order trades on Spark-Miden-v1 is achieved through the use of SWAPp (partial swap) notes. SWAPp notes have the following characteristics:

### SWAPp Characteristics
1. Can function as a regular SWAP note (as defined in the Miden-base repository).
2. Allow a user wallet to partially consume them, as long as the consuming wallet has the necessary SWAPp add-on procedures.
3. Are reclaimable by the creator.

### What is Partial Consumption of a SWAPp Note?

Partial consumption means that a SWAPp note allows a user who does not have sufficient liquidity to completely fill the SWAPp order to still execute their trade at the specified ratio of requested tokens in the SWAPp note. 

When partially filling a SWAPp note with liquidity L, the remaining liquidity L1 in the SWAPp note is added to the new outputted SWAPp note.

The process of partially filling a SWAPp note can continue N times until the liquidity in the SWAPp note is completely exhausted.

### Output of Partially Consuming a SWAPp Note

When partially consuming a SWAPp note, two notes are outputted:
1. A P2ID note with the requested asset for the SWAPp note creator.
2. A new SWAPp note with L1 liquidity of the asset being sold.

### Partial SWAPp fulfillment
![alt text](./docs/PartialFillSWAPp.svg)

### Running Tests:c
```
cargo test --test mock_integration
```
